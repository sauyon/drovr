use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::{AgentLaunch, load_config};
use crate::herdr::{AgentStatus, Herdr, PaneInfo, PaneState, PromptOutcome, SessionId};
use crate::run::{
    NotReapable, NotRehydratable, PassToken, Phase, PhaseStatus, REVIEWER_PREFIX, RunState,
    is_reviewer_phase_name, run_dir,
};
use crate::shell::shell_single_quote;

/// How often `phase_wait` polls the filesystem for the completion marker, and
/// how often `wait_agent_ready` polls the pane's agent status.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// How long `phase_send` waits for a freshly-spawned agent to reach its composer
/// before delivering the prompt (see `wait_agent_ready`).
const SEND_READY_TIMEOUT: Duration = Duration::from_secs(30);

/// The time a rehydrate guarantees to CONFIRMING a resumed session, on top of
/// whatever is left of the readiness budget.
///
/// The two waits share one deadline so a resume cannot hold the caller for
/// twice `SEND_READY_TIMEOUT`. The cost of sharing alone is that a slow LAUNCH
/// eats the budget and leaves confirmation a sliver — which makes "no session
/// seen" most likely exactly when the machine is loaded and the agent is
/// slowest to surface its id, i.e. it reports resumes that actually worked as
/// unconfirmed. A floor keeps the total bounded (35s worst case) while making
/// the step that decides whether the conversation came back impossible to
/// starve. Ten polls at `POLL_INTERVAL`.
const CONFIRM_FLOOR: Duration = Duration::from_secs(5);

/// How long `phase_send` gives herdr to confirm the agent actually started after
/// a prompt (and again after the submit nudge).
///
/// Must stay >= 5s: herdr only returns its precise `agent_prompt_stalled` verdict
/// once its own 5s no-state-change window has elapsed, and degrades to a bare
/// `timeout` below that. The headroom above 5s absorbs a slow first turn on a
/// cold agent without turning a healthy send into a spurious nudge.
const SEND_CONFIRM_TIMEOUT: Duration = Duration::from_secs(15);

/// Path of the completion marker a phase agent drops via `drovr phase done`.
/// `pub(crate)` so the code-review orchestrator can compute reviewer marker paths
/// without duplicating the naming.
pub(crate) fn done_marker(run: &str, phase: &str) -> PathBuf {
    run_dir(run).join(format!("{phase}.done"))
}

/// Environment variable carrying the current pass's token into the phase agent.
/// Set by `launch_in_pane`, read back by [`phase_done`] when the agent signals
/// completion. Not a secret — a nonce that only needs to differ between passes.
const PASS_ENV: &str = "DROVR_PASS";

/// Mint a token for one pass over a phase. Uniqueness is all that is required
/// (it is compared for equality, never parsed), and it must differ between two
/// passes in the same process, so: pid + nanos + a process-local counter. The
/// alphabet is deliberately `[0-9a-f-]` so the value is inert in a shell command
/// and in a marker file.
fn new_pass_token() -> PassToken {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Infallible by construction: `format!` of three integers is never empty.
    // `PassToken::new` is fallible because an EMPTY token is not representable
    // (see [`PassToken`]) — not because minting can fail.
    PassToken::new(format!(
        "{:x}-{:x}-{:x}",
        std::process::id(),
        nanos,
        SEQ.fetch_add(1, Ordering::Relaxed)
    ))
    .expect("a minted pass token is never empty")
}

/// Delete a phase's completion marker, treating "already gone" as success and
/// propagating every other failure with context. Callers depend on the marker
/// being ABSENT afterwards, so a swallowed error here is a silent false-complete.
fn remove_stale_marker(run_name: &str, phase: &str) -> io::Result<()> {
    let marker = done_marker(run_name, phase);
    match std::fs::remove_file(&marker) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io::Error::new(
            e.kind(),
            format!(
                "phase '{phase}': cannot clear stale completion marker {}: {e}. \
                 Refusing to start — the phase would appear complete the moment \
                 it is next awaited.",
                marker.display()
            ),
        )),
    }
}

/// Reject a phase name that could not address a real phase. Called by every entry
/// point that resolves a name into a filesystem path (`phase_done`, `phase_wait`,
/// `phase_send`, `collect`, and `phase_start`'s re-entry branch) — which is what
/// makes an unnamed or path-escaping phase unreachable even if one somehow exists
/// in `state.json`.
///
/// * Empty/whitespace: `Phase::default()` is representable with `name: ""`, and
///   refusing to address one keeps an unnamed phase unreachable through
///   `find_phase` without fighting the `..Default::default()` pattern.
/// * Path separators, `..`, and a leading `.`: the name is interpolated into
///   `<run_dir>/<phase>.done` and `<run_dir>/<phase>-HANDOFF.md`, so `../../x`
///   would place a run's marker outside its own directory.
///
/// **This is deliberately WEAKER than [`require_new_phase_name`], and the
/// asymmetry is the whole design.** A phase reaching this function ALREADY EXISTS
/// — it has a pane, a pass token and a live agent. Applying the creation alphabet
/// here would brick every phase an older drovr created under a name that was legal
/// then (an `angles` entry or a `<task>` containing a space): `phase done`,
/// `phase wait`, `phase send` and `collect` would all refuse the name the running
/// agent was launched under, with no migration path. Shell safety on this path is
/// carried by quoting at the emission sites — which it must be anyway, since run
/// names are unrestricted too.
fn require_phase_name(phase: &str) -> io::Result<()> {
    let bad = phase.trim().is_empty()
        || phase.starts_with('.')
        || phase.contains('/')
        || phase.contains('\\')
        || phase.contains("..")
        || phase.contains('\0');
    if bad {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "invalid phase name {phase:?}: must be non-empty and must not \
                 contain a path separator, '..', or a leading '.'"
            ),
        ));
    }
    Ok(())
}

/// Reject a name drovr is about to CREATE a phase under — `spawn_reviewer` (which
/// always appends), and `phase_start` ONLY when the name is not already in
/// `run.phases` (`phase_start` doubles as the re-entry path). Everything
/// [`require_phase_name`] rejects, plus an ALLOWLIST: `[A-Za-z0-9._:-]`.
///
/// An allowlist rather than a metacharacter denylist because a phase name is
/// interpolated into three different grammars and a denylist has to be right in
/// all of them, forever:
///
/// * a FILESYSTEM path, `<run_dir>/<phase>.done` and `<run_dir>/<phase>-HANDOFF.md`;
/// * a SHELL command handed to herdr's `pane run` (`DROVR_PHASE=<run>/<phase>`);
/// * a SHELL command drovr PRINTS for a human to paste (the `phase done` /
///   `phase start` remediations). That last one is the live delivery mechanism:
///   the user runs it themselves.
///
/// Quoting at every emission site is still necessary and still present — run and
/// task names are not restricted this way. What this buys is that no NEWLY
/// INTRODUCED phase name ever needs it: from here on, every name this build adds
/// to `run.phases` is one any command can mention literally.
///
/// The alphabet is everything drovr itself mints: pipeline names
/// (`implement-task-1`), reviewer names (`review:<task>:<iter>:<angle>` — hence
/// `:`), and version-ish suffixes. **A `<task>` or a configured `angle` with a
/// space or a metacharacter now fails here**, which is the point: `<task>` reaches
/// drovr from the review server's HTTP layer, where it is only checked for path
/// safety. See `docs/known-issues.md`.
fn require_new_phase_name(phase: &str) -> io::Result<()> {
    require_phase_name(phase)?;
    if !phase
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "invalid phase name {phase:?}: may use only letters, digits, '-', \
                 '_', '.' and ':' — a phase name is interpolated into file paths \
                 and into shell commands drovr suggests you run. (If this came from \
                 a `<task>` argument or a configured review `angle`, rename it: \
                 hyphens instead of spaces.)"
            ),
        ));
    }
    Ok(())
}

/// Refuse a name the OTHER phase list already answers to.
///
/// `run.phases` and `run.review_phases` are separate lists, and
/// `RunState::find_phase` searches `phases` first and returns the first match.
/// So the same name in BOTH is not a harmless duplicate — it is a phase that
/// resolves to the WRONG one, permanently. Every downstream lookup follows:
/// `require_pane_id` sends to the impostor's pane, `phase_done` reads the
/// impostor's pass token and applies the pipeline-only handoff contract to a
/// reviewer, and `code_review`'s wait polls the wrong pane. `phase_start` also
/// sweeps `<phase>.done` on its way in, so the collision destroys the real
/// phase's completion evidence before any of that.
///
/// The SAME list counts too, for the same reason: two entries under one name
/// means `find_phase` resolves to whichever was pushed first, so the second
/// reviewer's pane is unreachable. main's panel resume re-spawns under the same
/// `review:<task>:<iter>:<angle>` name and already drops the stale entry first
/// (`run.review_phases.retain(|p| p.name != phase)` — "so `find_phase` cannot
/// resolve to the replaced pane"), so merging it is safe; this makes that
/// ordering a requirement rather than a convention. Pinned by
/// `a_reviewer_must_be_de_registered_before_it_is_re_spawned`.
///
/// Checked at the two CREATION sites, before any side effect. Not reachable from
/// drovr's own naming (reviewer names carry a `review:` prefix pipeline names do
/// not use) — but nothing stops a human or a driver typing
/// `drovr phase start <run> <reviewer-name>`, and the recovery commands drovr
/// prints are bare `drovr phase start <run> <phase>`.
fn require_name_unclaimed(
    others: &[Phase],
    phase: &str,
    claimant: &str,
    run_name: &str,
) -> io::Result<()> {
    if !others.iter().any(|p| p.name == phase) {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "phase name {phase:?} is already taken by {claimant} in run \
             '{run_name}'. A name must identify ONE phase: registering it in both \
             lists makes every later lookup resolve to whichever is searched \
             first, silently rerouting that phase's pane, pass token and \
             completion marker."
        ),
    ))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn find_phase_idx(run: &RunState, phase: &str) -> Option<usize> {
    run.phases.iter().position(|p| p.name == phase)
}

/// The `drovr attach` suggestion every "nobody is at the pane" diagnostic ends
/// with. A helper, not a `format!` at each of the three sites, so the quoting
/// cannot be right in two of them and forgotten in the third.
fn attach_command(run_name: &str) -> String {
    format!("drovr attach {}", shell_single_quote(run_name))
}

/// The `drovr phase done` suggestion [`phase_done`]'s refusal and
/// [`phase_wait`]'s marker-mismatch diagnostic both print. `pass` is the token to
/// supply, or `None` to DROP the one being held (`env -u`) — the two remedies
/// differ, the quoting must not.
fn phase_done_command(run_name: &str, phase: &str, pass: Option<&PassToken>) -> String {
    let prefix = match pass {
        Some(want) => format!("{PASS_ENV}={}", shell_single_quote(want.as_str())),
        None => format!("env -u {PASS_ENV}"),
    };
    format!(
        "{prefix} drovr phase done {} {}",
        shell_single_quote(run_name),
        shell_single_quote(phase)
    )
}

/// Launch an agent invocation inside an already-chosen `pane`, tagged with
/// `DROVR_PHASE=<run>/<phase>` (single-quoted so a name with spaces or shell
/// metacharacters stays one literal word), then best-effort rename the pane to
/// `phase`. Focus is captured before and restored after, because `pane_run` /
/// `pane_rename` have no `--no-focus` flag and would otherwise steal focus from
/// the user.
///
/// `command` is the full agent invocation (e.g. `"claude"` for a pipeline phase,
/// or `"claude --permission-mode plan"` for a read-only reviewer). This helper is
/// PURE pane mechanics: it performs NO phase-list lookup and NO state mutation, so
/// callers stay in control of where (and whether) the phase is registered — a
/// reviewer name must never collide into a pipeline phase's pane.
///
/// `profile` is the `CLAUDE_CONFIG_DIR` to launch UNDER — a parameter rather
/// than a read of this process's environment, because a rehydrate must relaunch
/// under the profile the phase's pane originally authenticated with. The review
/// server is a long-lived daemon whose environment need not match the driver's,
/// and claude resolves a session beneath `$CLAUDE_CONFIG_DIR/projects/<escaped-cwd>/`,
/// so the wrong one silently finds no conversation at all. Fresh launches pass
/// [`agent_profile_env`], which is also what they record on the phase.
fn launch_in_pane<H: Herdr>(
    h: &H,
    run_name: &str,
    phase: &str,
    pane: &str,
    command: &str,
    pass: &PassToken,
    profile: Option<&str>,
) -> io::Result<()> {
    // Capture focus so the pane operations below don't steal it from the user.
    let prev_focus = h.focused_workspace();

    // Launch the agent through `env VAR=val …` rather than a bare `VAR=val cmd`
    // prefix: herdr's pane-level env (set at workspace/tab creation) does NOT
    // reach a `pane run` command — it only populates the pane's interactive
    // shell, which the run command doesn't inherit — and a bare leading
    // assignment isn't applied either. `env` sets the vars directly on the
    // launched process.
    //   * DROVR_PHASE tags the launch for the reflex hook (not a secret).
    //   * DROVR_PASS identifies THIS pass over the phase. It lives in the agent's
    //     environment precisely because that is immutable for the life of the
    //     agent: a previous pass's agent keeps ITS token no matter what later
    //     writes to state.json, so the marker it drops is always attributable.
    //   * CLAUDE_CONFIG_DIR selects the claude profile so the agent
    //     authenticates as the right account instead of falling back to
    //     ~/.claude. It is a path, not a secret, so inlining it is safe; a
    //     fresh launch propagates drovr's own environment (e.g. under a
    //     `claude-prof` profile) and a rehydrate propagates the one the phase
    //     recorded. Real secrets (API keys) are never inlined.
    // Values are single-quoted so spaces/metacharacters can't break out.
    let mut env_prefix = format!(
        "env DROVR_PHASE={} {PASS_ENV}={}",
        shell_single_quote(&format!("{run_name}/{phase}")),
        shell_single_quote(pass.as_str()),
    );
    if let Some(dir) = profile {
        env_prefix.push_str(&format!(" CLAUDE_CONFIG_DIR={}", shell_single_quote(dir)));
    }
    let full = format!("{env_prefix} {command}");
    // ⚠️ `pane_run` is the LAST fallible step, and that is load-bearing, not
    // incidental. `phase_start` treats an `Err` from this function as "no agent
    // was started" and CLOSES the pane it just created
    // (`discard_unlaunched_pane`). Everything after this line is therefore
    // best-effort by necessity: a `?` added below would make a cosmetic rename
    // failure kill a live agent mid-conversation.
    h.pane_run(pane, &full)?;
    // Cosmetic pane label; best-effort (a rename failure must not fail the phase).
    let _ = h.pane_rename(pane, phase);
    // Restore focus if a pane operation moved it.
    if let Some(prev) = prev_focus {
        let _ = h.workspace_focus(&prev);
    }
    Ok(())
}

/// Resolve a phase's pane id, searching `phases` then `review_phases` (via
/// `RunState::find_phase`) so `phase_send` can seed a reviewer pane registered in
/// `review_phases`, not just a pipeline phase.
///
/// **A phase whose pane was REAPED gets its own message, and that is not a
/// nicety.** `drovr phase send` into a finished phase is the pipeline's
/// documented re-entry path, and reaping made "the phase has no pane" a state a
/// driver reaches by doing the normal thing in the wrong order — starting the
/// next phase, which supersedes this one, and only then sending. A bare "phase
/// has no pane_id" names no cause and no way out; the recovery exists
/// (`drovr phase rehydrate` brings the pane back, resuming the agent's own
/// session where the backend offers one), so it is said here.
fn require_pane_id(run: &RunState, phase: &str) -> io::Result<String> {
    let p = run.find_phase(phase).ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, format!("phase not found: {phase}"))
    })?;
    if let Some(pane) = p.pane_id() {
        return Ok(pane.to_owned());
    }
    let reaped = p.is_reaped();
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        if reaped {
            format!(
                "phase '{}/{phase}' has no pane: drovr closed it when the run moved \
                 past this phase. Bring it back — resuming its agent's own session \
                 where the backend offers one — with `drovr phase rehydrate {q_run} \
                 {q_phase}`, then re-send.",
                run.name,
                q_run = shell_single_quote(&run.name),
                q_phase = shell_single_quote(phase),
            )
        } else {
            format!("phase has no pane_id: {phase}")
        },
    ))
}

/// Dispose of a pane this call opened but never managed to launch into.
///
/// **Not reaping.** Reaping closes a pane that did its job; this cleans up after
/// an operation that failed halfway. The pane is an orphan the instant the
/// launch errors: `phase_start` returns before `set_pane`, so nothing in
/// `state.json` names it, and a retry calls `tab_create` again — one dead tab
/// per attempt.
///
/// **Record BEFORE closing, and record even if the close works.** `drovr
/// cleanup` closes only the panes it can prove are drovr's, diffing
/// `Herdr::workspace_panes` against `drovr_pane_ids`. A pane drovr opened and
/// never recorded is therefore indistinguishable from one the human opened in
/// the run's workspace: cleanup leaves it alone *forever*, and — because
/// something foreign is present — refuses `workspace_close`, stranding the whole
/// run's workspace. So the retirement is what makes the record true regardless
/// of what the close does, and the close is a best-effort tidy on top. On
/// success it costs nothing: cleanup skips panes `workspace_panes` no longer
/// lists.
///
/// Both steps are best-effort and neither may mask the launch error the caller
/// is about to return — that error is what the human needs to see. The `save`
/// matters on its own: a retry runs in a fresh process, so a retirement that
/// only ever existed in memory is a retirement that never happened.
///
/// ⚠️ **Only safe because `launch_in_pane` cannot fail after `pane_run`
/// succeeds** — every step after it is `let _ =`. If that ever changes, this
/// closes a pane with a live agent in it. The invariant is pinned by a comment
/// at that `pane_run?`; do not weaken either half.
fn discard_unlaunched_pane<H: Herdr>(h: &H, run: &mut RunState, pane: &str) {
    run.retire_pane(pane);
    if let Err(e) = run.save() {
        eprintln!(
            "drovr: warning: could not record pane {pane} as drovr's after a failed \
             launch ({e}); `drovr cleanup` may leave it open"
        );
    }
    if let Err(e) = h.pane_close(pane) {
        eprintln!("drovr: warning: could not close pane {pane} after a failed launch: {e}");
    }
}

/// Take back a pane whose launch SUCCEEDED but whose registration could not be
/// persisted. Same three steps as [`discard_unlaunched_pane`], one crucial
/// difference: the agent in this one is **alive**.
///
/// A separate function rather than a shared one precisely because of that
/// difference. `discard_unlaunched_pane` rests on "`launch_in_pane` cannot fail
/// after `pane_run` succeeds", so nothing is ever running in the pane it
/// closes. Here something is, and closing it is a deliberate trade rather than
/// a tidy-up: a live pane that `state.json` does not name is the immortal-pane
/// bug — `drovr cleanup` closes only panes it can prove are drovr's (main's
/// `8173f03`), so an unrecorded one reads as the human's and is never closed,
/// while `attach`, `phase send` and the review UI are all blind to it. An agent
/// killed a second after it started is recoverable by retrying; a pane nothing
/// can see or close is not.
///
/// The retirement is recorded FIRST and separately, because it is the smaller
/// write: if it lands and the close then fails, cleanup can at least prove the
/// pane was drovr's.
///
/// ⚠️ **It is written onto a FRESH read of `state.json`, not onto the caller's
/// copy.** The caller's copy is exactly the one whose save just failed — it
/// carries the phase pointing at this pane, a new pass and a replaced agent
/// record. Saving that here would record a `pane_id` for a pane this function
/// is about to close, and a phase that claims a pane which no longer exists is
/// permanently unrehydratable: `rehydratable` answers `HoldsPane` forever and
/// nothing clears the registration. Disk-as-it-was plus the retirement is the
/// state that lets a retry start from the same place.
///
/// The caller's in-memory `RunState` is therefore left ahead of disk on
/// purpose. Every caller returns `Err` immediately, and `phase_rehydrate`
/// re-reads under its lock, so nothing acts on it.
fn surrender_unrecordable_pane<H: Herdr>(h: &H, run: &RunState, pane: &str) {
    match RunState::load(&run.name) {
        Ok(mut fresh) => {
            fresh.retire_pane(pane);
            if let Err(e) = fresh.save() {
                eprintln!(
                    "drovr: warning: could not record pane {pane} as drovr's ({e}); if \
                     closing it below also fails, `drovr cleanup` will treat it as yours \
                     and leave it open"
                );
            }
        }
        Err(e) => eprintln!(
            "drovr: warning: could not re-read run '{}' to record pane {pane} as drovr's \
             ({e}); if closing it below also fails, `drovr cleanup` will treat it as yours \
             and leave it open",
            run.name
        ),
    }
    if let Err(e) = h.pane_close(pane) {
        eprintln!(
            "drovr: warning: pane {pane} is running an agent that nothing records, and it \
             could not be closed either ({e}). Close it by hand (herdr pane close {pane}) \
             — until then it will not be cleaned up and it blocks closing the run's \
             workspace."
        );
    }
}

// ---------------------------------------------------------------------------
// Workspace recovery
// ---------------------------------------------------------------------------

/// The herdr label drovr gives a run's workspace. One function so `drovr new`
/// and the re-provisioning path here cannot drift — a replacement workspace
/// under a different label would be indistinguishable from a human's own in the
/// switcher, and in `cleanup`'s reasoning about whose panes are whose.
pub fn workspace_label(run_name: &str) -> String {
    format!("drovr:{run_name}")
}

/// The refusal for a run with no `project_dir` — the one piece of state drovr
/// genuinely cannot rebuild, since without it there is no directory to launch an
/// agent in and none to open a workspace in.
///
/// ONE function for every site that hits this, because the wording is the point.
/// It used to read "please recreate the run with `drovr new`" — advice that would
/// have discarded an approved spec, a plan and two tasks of committed work on the
/// run that exposed all of this. A run is not disposable just because one field
/// of it is missing, so this names the field and where to put it instead.
pub fn missing_project_dir_error(run_name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "run '{run_name}' has no project_dir (created before that field was \
             recorded), so drovr does not know which directory to work in. \
             Everything else about the run is intact: add \"project_dir\": \
             \"/path/to/checkout\" to {} and re-run this command.",
            run_dir(run_name).join("state.json").display()
        ),
    )
}

/// The refusal for a run the human filed away.
///
/// A shared constructor for the same reason [`missing_project_dir_error`] is one:
/// these are drovr's two hard refusals, both say "the run is fine, do this one
/// thing first", and a second wording of either is a place for the guidance to
/// drift. `code_review_run` reports it through its own outcome type but prints
/// this text.
pub fn archived_run_error(run_name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "run '{run_name}' is archived, and archiving destroyed its herdr \
             workspace. drovr will not rebuild one for a run you filed away — \
             nothing is lost, so Restore it (the Restore button in `drovr serve`'s \
             run list) and re-run this command."
        ),
    )
}

/// The workspace [`ensure_workspace`] has just guaranteed.
///
/// Reachable only if that guarantee is broken, so it reports a bug in drovr
/// rather than a state a user can be in — and specifically NOT the "recreate the
/// run" advice this area exists to retire. Shared by the two launch paths so the
/// one invariant is stated once.
fn workspace_or_bug(run: &RunState) -> io::Result<String> {
    run.workspace.clone().ok_or_else(|| {
        io::Error::other(format!(
            "internal: run '{}' still has no workspace after ensure_workspace",
            run.name
        ))
    })
}

/// What [`ensure_workspace`] had to do.
///
/// `Reprovisioned` deliberately does NOT carry the new workspace id: it is in
/// `run.workspace` by the time this is returned, and a copy beside it would be a
/// second place for the same fact to live — one the caller could pass to
/// [`healing_report`] mismatched with the run it is reporting on.
#[derive(Debug)]
pub enum WorkspaceHealing {
    /// The recorded workspace answered — nothing was touched.
    Intact,
    /// There was no live workspace, so one was created. Carries the names of the
    /// phases that were `Running` in the dead one, which the caller reports: their
    /// agents are gone with their context, and that is a fact about the run, not a
    /// detail of the repair.
    Reprovisioned { orphaned: Vec<String> },
}

/// Guarantee `run` has a live herdr workspace, creating one if it does not.
///
/// A workspace is disposable infrastructure — herdr destroys one the moment its
/// last pane closes, which `drovr cleanup` and ordinary pane-reaping both do —
/// while a run's phases, handoffs, commits and approved spec are not. Tying the
/// two together is what once made a 23-task run at task 3 unrecoverable through
/// drovr's own commands. So a missing workspace is repaired here rather than
/// reported as a terminal error, and "recreate the run" is never the advice.
///
/// HEALS AT THE POINT OF USE, not on load. `RunState::load` is a pure
/// deserialize with no herdr in reach, and its callers include `drovr status`,
/// `drovr list` and the always-on server's 2s list poll — none of which may
/// create infrastructure as a side effect of being read. The launch paths
/// (`phase_start`, `spawn_reviewer`, `resurrect`) are the ones that genuinely
/// need a workspace, and they are few enough to enumerate.
///
/// Saves `run` when it changes anything, so a caller that fails afterwards still
/// leaves the repair on disk instead of re-creating a second workspace next time.
///
/// NOT ATOMIC, deliberately. The check and the create are two calls with no lock
/// between them, and `state.json` has neither locking nor compare-and-swap (see
/// [`RunState::save`]). Two drovr processes acting on the same run at the same
/// moment can therefore both see the workspace as gone and both create one; the
/// loser's is never recorded, so nothing reaps it and it holds a live agent. That
/// needs two concurrent writers on one run, which drovr's single-writer
/// discipline forbids — and a lock here would close one instance of a race the
/// rest of the file has everywhere else, which is worse than an honest gap. It is
/// written up in `docs/known-issues.md`; the loser is labelled `drovr:<run>` like
/// any other, so a human can spot the duplicate in herdr's switcher.
pub fn ensure_workspace<H: Herdr>(h: &H, run: &mut RunState) -> io::Result<WorkspaceHealing> {
    if let Some(ws) = run.workspace.as_deref()
        && h.workspace_exists(ws)
    {
        return Ok(WorkspaceHealing::Intact);
    }
    // The human filed this run away, and `cleanup`/the Archive button destroyed
    // its workspace on purpose. Repairing that would make the destruction the only
    // thing that had been enforcing their decision — before re-provisioning
    // existed, `phase start` on an archived run failed precisely because nothing
    // recreated a closed workspace, and it would now succeed silently while the UI
    // still shows the run archived. Recovering from an ACCIDENT is this function's
    // job; overriding an intention is not.
    //
    // Checked after `workspace_exists`, so an archived run whose `workspace_close`
    // failed — a zombie, with live panes — still starts exactly as it does today.
    //
    // EVERYTHING FROM HERE WORKS ON A COPY, and `*run` is overwritten only once
    // the repair is safely on disk (see the commit point at the end).
    //
    // A half-repaired `RunState` — new workspace id, cleared pane ids, phases
    // demoted — is a run that LOOKS repaired and is not one, and any `save`
    // anywhere afterwards writes that fiction out. Returning `Err` beside a
    // comment saying "drop this" does not prevent it: the caller who would get it
    // wrong is exactly the one not reading the comment. Making the mutation
    // unreachable until it is durable does. Pinned by
    // `a_failed_repair_leaves_the_callers_run_state_untouched`.
    //
    // The clone costs one `RunState` per repair, and a repair only happens when a
    // workspace has actually vanished.
    let mut repaired = run.clone();

    // `refresh_archived` is THE way to consult this flag (see `RunState::archived`
    // for why disk is the authority). Adopting matters here specifically: the
    // `save_preserving_archived` below writes the copy in hand, so a guard that
    // read disk and left a stale `true` behind would re-archive, on its own
    // success path, the run it had just decided was not archived.
    if repaired.refresh_archived().map_err(|e| {
        io::Error::new(
            e.kind(),
            format!(
                "run '{}': cannot read {} to check whether it was archived, so drovr \
                 will not create a workspace for it: {e}",
                run.name,
                run_dir(&run.name).join("state.json").display()
            ),
        )
    })? {
        return Err(archived_run_error(&run.name));
    }
    // A workspace created without a cwd opens wherever herdr defaults to — the
    // near-miss that nearly had a phase agent editing an unrelated repo — so a
    // run with no project_dir gets no workspace at all.
    if repaired.project_dir.is_empty() {
        return Err(missing_project_dir_error(&run.name));
    }
    let ws = h.workspace_create(&workspace_label(&run.name), &repaired.project_dir)?;

    // Every pane id in `run` named a pane in the workspace that is gone, so all
    // of them are now dangling. Dropping them is not tidiness: a stale id is what
    // `phase_send` would aim at and what `cleanup` would try to close.
    let mut orphaned = Vec::new();
    for phase in repaired
        .phases
        .iter_mut()
        .chain(repaired.review_phases.iter_mut())
    {
        phase.forget_dangling_pane();
        phase.herdr_session = None;
        // A `Done` phase does not care — its work is in the handoff and in git.
        // A `Running` one is the real question, and this is the answer: its agent
        // died with the workspace, taking its context, so the phase is `Failed`
        // and not `Running`. Silently respawning would present work nobody is
        // doing as still in flight, which is the same class of lie as `resurrect`
        // advertising a resume it never restored.
        if phase.status == PhaseStatus::Running {
            phase.status = PhaseStatus::Failed;
            orphaned.push(phase.name.clone());
        }
    }
    // Retired panes are drovr's to reap, and there is nothing left to reap.
    repaired.retired_panes.clear();
    repaired.workspace = Some(ws.id.clone());
    // The replacement's root shell pane is handed to the next phase exactly as
    // `drovr new`'s is, so recovery leaves no idle shell behind either.
    repaired.root_pane = Some(ws.root_pane);

    // ORDER: create, mutate a copy, save — and hand the workspace BACK if the save
    // fails.
    //
    // Persisting first is not available: the id to persist only exists once herdr
    // has created it. So the window between the two is real, and the question is
    // only what a failure in it leaves behind. Without the reclaim it leaves the
    // worst of both: `state.json` still names the DEAD workspace, so the next
    // attempt creates a second replacement, while the first stands in the human's
    // switcher labelled `drovr:<run>` with nothing pointing at it — the
    // duplicate-workspace failure this file documents for concurrent writers, now
    // reachable from a single one on a transient ENOSPC.
    //
    // Closing it is safe and exact: we created it microseconds ago, nothing has
    // been launched into it (`ensure_workspace` runs before any `pane_run`), and
    // it is not the id anything has recorded.
    if let Err(save_err) = repaired.save_preserving_archived() {
        return Err(reclaim_unrecorded_workspace(h, &ws.id, &run.name, save_err));
    }

    // THE COMMIT POINT. Everything above touched `repaired`, so until this line
    // the caller's run is exactly what it handed in — including on every error
    // path above, none of which can leave it looking repaired. Reached only once
    // the repair is durable, which is what makes the two agree.
    *run = repaired;
    Ok(WorkspaceHealing::Reprovisioned { orphaned })
}

/// Give back a workspace drovr created but could not record, and describe what
/// happened. Always returns an `Err` for the caller to propagate — this is a
/// failure path, and the reclaim is cleanup, not recovery.
///
/// If the close ALSO fails there is nothing left to try, so the message names the
/// id and the label so a human can close it by hand. Reporting the save failure as
/// if nothing were left behind would be the lie this whole area exists to remove.
fn reclaim_unrecorded_workspace<H: Herdr>(
    h: &H,
    workspace: &str,
    run_name: &str,
    cause: io::Error,
) -> io::Error {
    let stranded = match h.workspace_close(workspace) {
        Ok(()) => String::new(),
        Err(close_err) => format!(
            " — and it could not be closed again either ({close_err}), so herdr still \
             holds workspace {workspace} (labelled `{}`) with nothing pointing at it; \
             close it by hand",
            workspace_label(run_name)
        ),
    };
    io::Error::new(
        cause.kind(),
        format!(
            "run '{run_name}': could not record the replacement herdr workspace \
             ({cause}), so the repair did not stick and was rolled back{stranded}"
        ),
    )
}

/// What a repair cost, in the words every path reports it in.
///
/// One formatter rather than one per caller: `phase_start`/`spawn_reviewer` warn
/// on stderr and `resurrect` returns a report on stdout, but a driver who reads
/// both must not have to work out whether two differently-worded messages
/// describe the same event. Lines are newline-terminated; the caller frames them.
pub fn healing_report(run: &RunState, orphaned: &[String]) -> String {
    let mut out = format!(
        "run '{}' had no live herdr workspace; created {} in {}\n",
        run.name,
        run.workspace.as_deref().unwrap_or("(unrecorded)"),
        run.project_dir
    );
    if !orphaned.is_empty() {
        out.push_str(&format!(
            "these phases were Running in the old workspace and their agents are gone \
             with their context — marked FAILED, restart the one you want: {}\n",
            orphaned.join(", ")
        ));
    }
    out
}

/// Repair `run`'s workspace and warn on stderr about what the repair cost.
///
/// Split from [`ensure_workspace`] so the pure state transition stays testable
/// without capturing output. `resurrect` does its own framing (the repair is its
/// headline, not a warning beside a launch) but shares [`healing_report`], so the
/// wording cannot drift between them.
fn ensure_workspace_reporting<H: Herdr>(h: &H, run: &mut RunState) -> io::Result<()> {
    if let WorkspaceHealing::Reprovisioned { orphaned } = ensure_workspace(h, run)? {
        for line in healing_report(run, &orphaned).lines() {
            eprintln!("drovr: {line}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Spawn a persistent claude pane for `phase`, record pane_id + herdr_session,
/// set status Running, and save.  If the phase already exists (e.g. resume) its
/// entry is reused; if not, it is appended.
pub fn phase_start<H: Herdr>(
    h: &H,
    run: &mut RunState,
    phase: &str,
    seed: Option<&Path>,
) -> io::Result<()> {
    // `phase_start` is BOTH the creation path and the documented RE-ENTRY path
    // (`token_lost_message` prints `drovr phase start <run> <phase>` as the
    // recovery for a vanished token, and `skills/pipeline` re-enters this way).
    // The strict alphabet therefore applies to a name being INTRODUCED, not to
    // one already in `state.json` — gating the whole function would brick exactly
    // the legacy-named phases [`require_phase_name`]'s weaker rule exists to keep
    // working, on the recovery drovr itself suggests.
    if find_phase_idx(run, phase).is_none() {
        require_new_phase_name(phase)?;
        // A reviewer-shaped name is a REVIEWER's, whether or not one exists
        // yet. The membership check below only sees reviewers already spawned,
        // so without this a pipeline phase could be created under a panel name
        // — and it then passed the rehydrate gate's reviewer refusal (which
        // scanned `review_phases`), rendered a ⟳, and relaunched as a WRITER
        // with no findings channel. One predicate, both gates: see
        // `crate::run::is_reviewer_phase_name`.
        if is_reviewer_phase_name(phase) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "phase name {phase:?} is reserved for review-panel agents (anything \
                     starting with {REVIEWER_PREFIX:?}), which only `drovr code-review run` \
                     spawns — it writes them into the run's reviewer list and gives them a \
                     findings channel, neither of which `phase start` can do. Pick another \
                     name."
                ),
            ));
        }
        // A name a REVIEWER already answers to is not free, even though
        // `find_phase_idx` (which searches `phases` only) says it is.
        require_name_unclaimed(&run.review_phases, phase, "a reviewer", &run.name)?;
    } else {
        require_phase_name(phase)?;
    }
    // Checked here as well as inside `ensure_workspace`, and for a different
    // reason: `project_dir` is this phase's cwd and its `--add-dir` guard, so it
    // is required even when the recorded workspace is perfectly alive. Both sites
    // raise the SAME error, so which one fires first is not something a reader has
    // to know.
    if run.project_dir.is_empty() {
        return Err(missing_project_dir_error(&run.name));
    }
    let cwd = run.project_dir.clone();

    // BEFORE the pass is minted and this phase is marked Running: a repair
    // demotes every Running phase to Failed, and running it afterwards would
    // demote the one we are starting. It also clears this phase's `pane_id`, so
    // the pane selection below correctly falls through to the new root pane
    // instead of aiming at a pane in the destroyed workspace.
    ensure_workspace_reporting(h, run)?;

    // Re-entering a phase means: mint a new pass, record it, and drop the previous
    // pass's completion marker — in that order (see the ORDER MATTERS note below).
    // Nothing else sweeps `<phase>.done`.
    //
    // The sweep RAISES on any failure other than "already gone": callers depend on
    // the marker being absent afterwards. It is the first line of defence, not the
    // only one — the previous pass's agent is still alive and can recreate the
    // marker a moment later, and the token is what makes that recreation
    // identifiable.
    // Mint this pass's token before the launch — it has to go into the agent's
    // environment, which is fixed at exec time.
    let pass = new_pass_token();

    // ORDER MATTERS: persist the new pass FIRST, destroy the old marker SECOND.
    //
    // Both steps can fail, and the rule is that no failure may leave a state in
    // which `phase_wait` reports a completion for a pass whose agent is not
    // running. Persist-then-sweep satisfies it on every path:
    //   * save fails  → nothing was touched. The phase keeps its old status and
    //     old marker, which are consistent with each other and describe a pass
    //     that genuinely did finish. `phase_start` returns Err (exit 1).
    //   * sweep fails → the phase already holds the NEW token while the marker
    //     still holds the OLD one, so `phase_wait` rejects the marker either way:
    //     a fresh wait times out, and a wait that was already running on the old
    //     token reports `Superseded` (this re-entry is exactly what superseded it).
    // The reverse order has a hole: sweep succeeds, save fails, and the phase is
    // left `Done` from the previous pass with no agent running.
    //
    // Committing `Running` + the new token here (rather than clearing `pass`) is
    // also what keeps the launch failure path fail-CLOSED: a token no agent holds
    // rejects every marker. Clearing to `None` would drop the phase into the
    // legacy bucket while the previous pass's agent is still alive.
    if let Some(i) = find_phase_idx(run, phase) {
        run.phases[i].status = PhaseStatus::Running;
        run.phases[i].pass = Some(pass.clone());
        // The ONE place a captured session is discarded, and it belongs HERE,
        // beside the token that invalidates it — not after the launch. Minting a
        // new pass is what makes the recorded id inapplicable: whatever happens
        // next, this phase is no longer the conversation that id names. Clearing
        // it after `launch_in_pane` would skip exactly the case that needs it —
        // a launch that FAILS still leaves the new `pass` and `Running` on disk
        // (both are persisted right here, before the pane is touched), so the
        // phase would sit on a pass no agent ever ran under while advertising
        // the *previous* pass's conversation as resumable.
        //
        // Conservative in the right direction: the next poll captures the new id
        // the moment an agent attaches, and until then the honest answer is
        // "none", which degrades a rehydrate to a reseed rather than to the
        // wrong conversation. `tab_id` is untouched — a relaunch reuses the
        // pane, so it reuses the tab.
        // Replace the record rather than clearing a field: a new pass is a new
        // agent process, so the backend/profile stay but the conversation the
        // old id names is no longer this phase's. `PhaseAgent` has no way to
        // clear a session on purpose — that is `record_session`'s whole point —
        // so the launch record is rebuilt from what it already knows.
        if let Some(old) = run.phases[i].pane_agent() {
            let (backend, profile) = (old.backend().to_owned(), old.profile().map(str::to_owned));
            run.phases[i].record_launch(backend, profile);
        }
        // Preserving, like every other writer holding a snapshot the human may
        // have archived under: it only rescues `archived`, so it cannot disturb
        // the pass-token write above.
        run.save_preserving_archived()?;
    }
    remove_stale_marker(&run.name, phase)?;

    // (see `discard_unlaunched_pane` for what happens if the launch below fails)
    //
    // Pick the pane this phase's `claude` will run in, WITHOUT splitting a new
    // pane beside an empty shell:
    //   * a restarting phase reuses its own recorded pane;
    //   * every other phase gets its own fresh tab (whose auto shell pane it
    //     reuses).
    //
    // `run.root_pane` is deliberately NOT a candidate. The first phase used to
    // claim it, which put a phase agent in the pane that anchors the whole
    // workspace: closing that phase's tab would close the workspace out from
    // under every other phase, and `drovr attach` / the review UI would have no
    // stable thing to point at. The root shell now stays idle for the run's
    // lifetime, and every phase tab is independently closeable. It is still
    // drovr's pane — `drovr cleanup` reclaims it (`drovr_pane_ids` lists it
    // first) — just never an agent's.
    let existing_pane =
        find_phase_idx(run, phase).and_then(|i| run.phases[i].pane_id().map(str::to_owned));
    // Whether THIS call opened the pane. A pane we did not create is not ours to
    // discard when the launch fails — it is the phase's own, from a previous
    // pass, and the retry wants it.
    let mut created_pane = false;
    let target_pane = if let Some(pane) = existing_pane {
        pane
    } else {
        created_pane = true;
        // `ensure_workspace` above either found a live workspace or created one.
        h.tab_create(&workspace_or_bug(run)?, phase, &cwd)?
    };

    // Use the backend captured by `drovr new`, so every phase stays on the
    // caller's agent even when later commands run from a plain shell.
    let cfg = load_config()?;
    let agent = run.agent.as_deref().unwrap_or("claude");
    // No MCP config: the findings channel exists only for read-only reviewers.
    let launch = cfg.launch(agent, &cwd, false, None)?;
    // Read ONCE: the profile inlined into the agent's environment and the one
    // recorded on the phase have to be the same value, or a later rehydrate
    // resumes under a profile this pane never authenticated with.
    let profile = agent_profile_env();
    if let Err(e) = launch_in_pane(
        h,
        &run.name,
        phase,
        &target_pane,
        launch.command(),
        &pass,
        profile.as_deref(),
    ) {
        if created_pane {
            discard_unlaunched_pane(h, run, &target_pane);
        }
        return Err(e);
    }

    // Find existing phase or append a new one
    let idx = match find_phase_idx(run, phase) {
        Some(i) => i,
        None => {
            run.phases.push(Phase::new(phase));
            run.phases.len() - 1
        }
    };

    let seed_str = seed.map(|p| p.to_string_lossy().into_owned());
    run.phases[idx].handoff_doc = seed_str;
    // pane_id only — herdr_session is not used for cleanup, which closes panes by
    // id (`close_run_panes` in main.rs)
    run.phases[idx].herdr_session = None;
    run.phases[idx].set_pane(target_pane);
    run.phases[idx].pass = Some(pass);
    run.phases[idx].status = PhaseStatus::Running;
    // Backend + profile as one value: both are facts about THIS launch, and the
    // session captured later is only meaningful alongside the backend.
    // NOTE: the session is cleared where the new pass is PERSISTED, above — not
    // here. See the comment there; clearing at this point would miss the
    // launch-failure path.
    run.phases[idx].record_launch(launch.backend(), profile);

    // `pane_id` is recorded here because it is how `drovr cleanup` — and now
    // reaping — knows which panes are drovr's and which are the human's.
    //
    // `save_preserving_archived`, not `save`: the caller has held this state since
    // before the pane was launched, and the human may have archived the run from
    // the web UI in between. Writing a stale `archived: false` back would
    // un-archive a run whose workspace is already destroyed.
    run.save_preserving_archived()?;

    // ⭐ THE SUPERSESSION TRIGGER, and its position is the whole design.
    //
    // AFTER the launch and after the save, because this is the first moment the
    // run has provably moved past its finished phases — and only a launch that
    // worked is evidence of that. On every earlier line the phase being started
    // might still fail, and reaping on the strength of an attempt would close
    // the panes of a run that has not moved anywhere.
    //
    // NOT on completion, which is the shape this looks like and must not be:
    // `skills/pipeline` promises the driver that a pane outlives `drovr phase
    // done`, and the implement↔review loop re-enters the SAME pane with `phase
    // send` and no `phase start`. Reaping when a phase finishes would kill
    // drovr's core quality loop on its first iteration.
    //
    // Best-effort throughout: neither call returns anything, so no failure here
    // can turn a started phase into a failed command.
    //
    // The sweep runs FIRST, and that ordering is worth a line: `reap_superseded`
    // retires every pane it closes, and sweeping afterwards would spend a herdr
    // round trip re-establishing that a pane drovr closed a moment ago is
    // indeed gone. Going first, it sees only the debris that was already there.
    if cfg.reap_finished_panes {
        reap_retired(h, run);
        reap_superseded(h, run, phase);
    }
    Ok(())
}

/// Spawn a read-only reviewer agent pane for `phase` (a
/// `review:<task>:<iter>:<angle>` name), registering it in `run.review_phases`
/// (NOT `run.phases`, so reviewers never pollute the pipeline's progress).
///
/// `launch_command` is the composed agent invocation including its read-only flag,
/// e.g. `"claude --permission-mode plan"` (built by the caller from
/// `Config::reviewer_launch`). The pane runs
/// `DROVR_PHASE='<run>/<phase>' <launch_command>` via the shared `launch_in_pane`
/// helper (DROVR_PHASE single-quoted; focus captured/restored).
///
/// Reviewers ALWAYS get a fresh tab in the run workspace — like every pipeline
/// phase, they never touch `run.root_pane` — so a workspace is required;
/// this errors clearly if `run.workspace` is `None`.
///
/// `seed` (if any) is recorded on the phase's `handoff_doc` for the caller to
/// inject via `phase_send`; it is NOT placed on the command line.
///
/// `launch` carries the composed invocation AND the backend it was composed
/// from, as one value. They used to be two `&str` parameters and a caller got
/// them out of sync, recording a backend the pane was not running — which made
/// session capture check the wrong agent and silently record nothing. A reviewer
/// is chosen by `Config::review_agent_for` and legitimately differs from the
/// run's backend, so the two really can disagree; now they cannot.
pub fn spawn_reviewer<H: Herdr>(
    h: &H,
    run: &mut RunState,
    phase: &str,
    seed: Option<&Path>,
    launch: &AgentLaunch,
) -> io::Result<()> {
    require_new_phase_name(phase)?;
    // Reviewers always APPEND, so BOTH lists must be clear: `next_iter` keeps
    // reviewer names unique across passes, but a resume re-spawning in place must
    // de-register first (main's does).
    require_name_unclaimed(&run.phases, phase, "a pipeline phase", &run.name)?;
    require_name_unclaimed(&run.review_phases, phase, "a reviewer", &run.name)?;
    // Same guard as phase_start: a run with no project_dir can't anchor the
    // workspace-root guard (or the tab cwd), so refuse rather than launch a
    // reviewer with `--add-dir ''`.
    if run.project_dir.is_empty() {
        return Err(missing_project_dir_error(&run.name));
    }

    // Reviewers can't reuse the pipeline root pane; they need their own tab, which
    // requires a workspace — so a missing or destroyed one is repaired here too.
    // `code-review run` spawns several reviewers in a loop, which is precisely
    // when a workspace is most likely to have been emptied by pane-reaping.
    ensure_workspace_reporting(h, run)?;
    let ws = workspace_or_bug(run)?;

    // Sweep any `<phase>.done` this name already answers to, BEFORE launching —
    // `phase_start` does the same, for the same reason, and the reviewer case is
    // not hypothetical. `next_iter` keeps a FRESH reviewer's name unique, but a
    // panel RESUME respawns an angle in place: same task, same iteration, same
    // name, so the marker the reviewer being replaced left behind would make its
    // replacement read as "finished without delivering" from the first poll.
    // `code_review_run` already clears that reviewer's findings file for exactly
    // this reason; this is the other half.
    //
    // `?`, never `let _ =`: the wait loop depends on the marker being absent, so
    // a swallowed failure launches a reviewer that is complete before it starts.
    // Before the `tab_create` below, so a sweep that fails leaves no pane behind.
    remove_stale_marker(&run.name, phase)?;

    // A fresh tab (with its auto shell pane) in the run workspace — never the root
    // pane. `tab_create` is `--no-focus`; `launch_in_pane` handles focus around the
    // launch itself.
    let pane = h.tab_create(&ws, phase, &run.project_dir)?;
    // Reviewers get a pass token too. They never collide with a previous pass
    // (their names embed `iter`, and `next_iter` takes max+1 over an append-only
    // `review_phases`), so this is uniformity rather than a fix — but it means
    // every marker in the run dir is attributable to the launch that produced it.
    let pass = new_pass_token();
    // Read ONCE, for the same reason `phase_start` does: the profile inlined
    // into the agent's environment and the one recorded on the phase must be
    // the same value.
    let profile = agent_profile_env();
    // Same orphan hole as `phase_start`'s, and the same fix: registration below
    // happens only after the launch succeeds, so a failure here leaves a pane
    // nothing records — which `drovr cleanup` then protects as the human's,
    // forever, while it blocks `workspace_close` for the whole run. The tab is
    // always ours (`tab_create` is three lines up, unconditionally), so there is
    // no "did this call create it" question to answer here.
    if let Err(e) = launch_in_pane(
        h,
        &run.name,
        phase,
        &pane,
        launch.command(),
        &pass,
        profile.as_deref(),
    ) {
        discard_unlaunched_pane(h, run, &pane);
        return Err(e);
    }

    // Register the reviewer in `review_phases` only. The seed path rides on
    // handoff_doc for later `phase_send` injection, mirroring `phase_start`.
    let seed_str = seed.map(|p| p.to_string_lossy().into_owned());
    // Built field by field rather than as a literal: `pane_id` and `reaped` are
    // private (they are a lifecycle pair — see `Phase::set_pane`), so a literal
    // cannot name them. Every field is still decided here deliberately, which is
    // what a literal used to buy:
    //   * `agent` — backend + profile, recorded now because the launch is the
    //     only moment either is knowable, and a reviewer is resumed the same way
    //     a phase is. The backend is NOT `run.agent`: see `PhaseAgent`.
    //   * `tab_id`, and the session inside `agent` — genuinely unknown here:
    //     `tab_create` returns a PANE id, and the agent has not attached yet.
    //     Both are captured by `poll_phase_pane`, from the readiness gate in the
    //     `phase_send` that seeds this reviewer and from `code_review`'s wait
    //     loop thereafter. Left at their defaults on purpose.
    //   * `reaped` — `set_pane` clears it; a pane just created is not reaped.
    let mut reviewer = Phase::new(phase);
    reviewer.status = PhaseStatus::Running;
    reviewer.handoff_doc = seed_str;
    reviewer.set_pane(pane);
    reviewer.pass = Some(pass);
    reviewer.record_launch(launch.backend(), profile);
    run.review_phases.push(reviewer);
    // Preserving, as in `phase_start`: this runs once per angle inside
    // `code_review_run`'s spawn loop, each iteration a herdr round trip, so an
    // Archive click lands here far more easily than the name suggests.
    run.save_preserving_archived()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Rehydrate
// ---------------------------------------------------------------------------

/// What a [`phase_rehydrate`] actually did — the difference between "your
/// conversation is back" and "a fresh agent is reading the notes", which the
/// human must not have to guess at.
#[derive(Debug, PartialEq, Eq)]
pub enum RehydrateOutcome {
    /// The recorded session was resumed: same conversation, same agent — with
    /// herdr reporting that session id back on the new pane, not merely an
    /// agent that came up there.
    Resumed,
    /// No session was recoverable, so a fresh agent was launched and the
    /// phase's seed re-sent. The conversation is gone; the written record is
    /// what the new agent has.
    Reseeded,
    /// The pane is back, but the agent in it was NOT confirmed to have this
    /// phase's context.
    ///
    /// **This is the only outcome that is not a success**, and the CLI gives it
    /// exit 2 (see `main::rehydrate_report`). Every path that cannot prove the
    /// agent is up and informed lands here rather than in `Resumed`/`Reseeded`.
    Incomplete(Unfinished),
}

/// Which of the five genuinely different things went wrong.
///
/// This was a `String` note, and a `String` is the wrong type for it: the note
/// is what a human reads, but the *classification* is what a caller acts on —
/// the HTTP layer, a driver deciding whether to retry, anything that needs to
/// tell "the agent never came up" apart from "it came up in the wrong
/// conversation". Recovering that by matching on prose is a caller that will
/// eventually get it wrong. The prose is now derived from the variant
/// ([`Unfinished::note`]) rather than the variant from the prose.
#[derive(Debug, PartialEq, Eq)]
pub enum Unfinished {
    /// The agent never reported a started status on its new pane. It may be
    /// parked on a first-run or permission prompt, or it may never have
    /// attached at all.
    NeverReady {
        pane: String,
        waited: Duration,
        /// Whether this was a resume; decides what did NOT happen.
        resuming: bool,
        /// Whether there was a seed to send, so a reseed's note does not blame
        /// a delivery that was never attempted.
        had_seed: bool,
    },
    /// The agent came up carrying a DIFFERENT session. The conversation did not
    /// come back — a stale id makes the backend start a FRESH one, which looks
    /// perfectly healthy from the outside.
    ///
    /// **The pane has been surrendered** ([`surrender_misattributed_pane`]),
    /// because a different session is positive evidence that the phase's record
    /// does not describe it. That fact is carried by the VARIANT rather than by
    /// a flag beside it: the arm that decides to close the pane is the arm that
    /// builds this, so the note cannot describe a pane that is still there.
    ResumeContradicted {
        pane: String,
        expected: SessionId,
        /// What herdr reported instead. Not an `Option` — a session that was
        /// never seen is [`Unfinished::ResumeUnobserved`], a different state.
        observed: SessionId,
    },
    /// The agent came up, and herdr never reported which session it is in.
    ///
    /// **The pane is still there.** drovr saw nothing, and nothing is not
    /// evidence — the agent may be perfectly resumed and merely slow to
    /// surface its id. Epistemically the same state as
    /// [`Unfinished::NeverReady`] with `resuming: true`, and it takes the same
    /// branch: nothing is destroyed, and the operator is sent to the pane.
    ResumeUnobserved { pane: String, expected: SessionId },
    /// A fresh agent is up, and this phase has no seed document to give it.
    NoSeed { pane: String },
    /// A fresh agent is up, and its seed could not be delivered.
    SeedUndelivered {
        pane: String,
        seed: String,
        error: String,
    },
}

impl Unfinished {
    /// The pane a human should look at. Every arm has one — the pane is always
    /// back; that is what makes these outcomes rather than errors.
    pub fn pane(&self) -> &str {
        match self {
            Unfinished::NeverReady { pane, .. }
            | Unfinished::ResumeContradicted { pane, .. }
            | Unfinished::ResumeUnobserved { pane, .. }
            | Unfinished::NoSeed { pane }
            | Unfinished::SeedUndelivered { pane, .. } => pane,
        }
    }

    /// The line printed verbatim to a human: what did not happen, and the one
    /// command that moves it forward.
    pub fn note(&self, run_name: &str, phase: &str) -> String {
        let pane = shell_single_quote(self.pane());
        match self {
            Unfinished::NeverReady {
                waited,
                resuming,
                had_seed,
                ..
            } => {
                // Sub-second timeouts render as ms, so an injected test timeout
                // does not print a misleading "within 0s" — the same rendering
                // `phase_send` uses.
                let waited = if waited.as_secs() >= 1 {
                    format!("{}s", waited.as_secs())
                } else {
                    format!("{}ms", waited.as_millis())
                };
                // Three genuinely different things did not happen, and saying
                // the wrong one is how a human debugs the wrong problem. The
                // no-seed case is not hypothetical: `phase_start` takes
                // `seed: Option<&Path>`.
                let why = if *resuming {
                    "Its recorded session may no longer resolve — the conversation was NOT restored"
                } else if !*had_seed {
                    "It has no recorded seed document either, so it has no context at all"
                } else {
                    "Its seed was NOT re-sent"
                };
                // THE PANE IS STILL THERE on every `NeverReady` — drovr
                // observed nothing, and nothing is not grounds for closing it
                // (see the arm in `phase_rehydrate`). So the guidance is the
                // same shape on both paths: go and look at it. Only the
                // resume path adds what a retry will run into, because there
                // the pane blocks one until it is gone.
                let blocked = if *resuming {
                    " Until that pane is gone, rehydrating this phase again \
                     will refuse with \"still holds pane\" — which is correct \
                     while the agent is still coming up, and wrong once it has \
                     exited. `drovr phase reap` clears it either way: it closes \
                     the pane if it is still there, and drops the registration \
                     if herdr has already lost it."
                } else {
                    ""
                };
                format!(
                    "the agent for phase '{phase}' did not become ready within {waited} on \
                     pane {p}. {why}. It may be parked on a first-run or permission prompt, \
                     or it may never have attached at all. Look with herdr pane read {pane} \
                     — that works either way, where `herdr agent attach` needs an agent to \
                     already be there — then `drovr phase send {run_name} {phase} …`.{blocked}",
                    p = self.pane(),
                )
            }
            Unfinished::ResumeContradicted {
                expected, observed, ..
            } => format!(
                "the agent for phase '{phase}' came up, but it is NOT the conversation you \
                 asked for: drovr resumed session '{}' and it reports session '{}' instead. \
                 A recorded id that no longer resolves makes the backend start a FRESH \
                 conversation, which looks healthy from the outside. That agent was closed \
                 again rather than left running under this phase's name, so the phase is \
                 unchanged and still holds the session id — rehydrating again will retry \
                 it. If it keeps failing the conversation is gone: `drovr phase start \
                 {q_run} {q_phase} <seed>` starts the phase fresh from the written record.",
                expected.as_str(),
                observed.as_str(),
                q_run = shell_single_quote(run_name),
                q_phase = shell_single_quote(phase),
            ),
            // NOT the contradicted wording, and the difference is the pane.
            // Nothing was observed, so nothing was destroyed — promising a
            // retry here would promise one `HoldsPane` is about to refuse.
            Unfinished::ResumeUnobserved { expected, .. } => {
                format!(
                    "the agent for phase '{phase}' came up on pane {p}, but drovr never saw \
                     which session it is in — it asked for '{}' and herdr reported no \
                     session at all before the wait ran out. That is not proof the resume \
                     failed: the id can surface a moment after the agent does. The pane is \
                     still there and still holds the phase — look with herdr pane read \
                     {pane}. Until that pane is gone, rehydrating this phase again will \
                     refuse with \"still holds pane {p}\"; `drovr phase reap {q_run} \
                     {q_phase}` is what clears that, by closing the pane.",
                    expected.as_str(),
                    p = self.pane(),
                    q_run = shell_single_quote(run_name),
                    q_phase = shell_single_quote(phase),
                )
            }
            Unfinished::NoSeed { .. } => format!(
                "phase '{phase}' has no recorded seed document, so the fresh agent was \
                 launched with no context. Send it some: `drovr phase send {q_run} \
                 {q_phase} '<what to do>'`",
                q_run = shell_single_quote(run_name),
                q_phase = shell_single_quote(phase),
            ),
            Unfinished::SeedUndelivered { seed, error, .. } => format!(
                "the fresh agent for phase '{phase}' is up, but its seed could not be \
                 delivered ({error}). Re-send it: `drovr phase send {run_name} {phase} \
                 '<read {seed}>'`"
            ),
        }
    }
}

/// Take the exclusive lock that serializes the commands which move a run's
/// panes around, held for as long as the returned file lives.
///
/// **Two holders, one lock, on purpose.** `phase_rehydrate` brings a phase back
/// on a new pane and `phase_reap` closes one and drops the registration — they
/// are the same read-modify-write over the same field, in opposite directions,
/// and reaping a phase a rehydrate is bringing back (or the reverse) is a race
/// that ends with a live pane nothing records. One file (`run.lock`, not the
/// `rehydrate.lock` it was named while rehydrate was the only holder), because
/// two locks taken in two orders is how a deadlock is built.
///
/// **Why a FILE lock.** The racers are separate processes — the HTTP handler
/// deliberately shells out to `current_exe()` so the CLI stays the sole writer
/// of `state.json` — so nothing in this address space can serialize them. The
/// kernel holds an advisory lock for a process and drops it however that
/// process dies, so a crashed holder never leaves a claim anyone has to judge
/// stale.
///
/// **Why it fails rather than waits.** The holder may be inside a 30-second
/// readiness wait, and a queued second rehydrate would then either duplicate
/// the first (if it did not re-read) or refuse anyway (if it did). Refusing now
/// says the true thing immediately and keeps the HTTP worker free. A reap
/// refused this way is best-effort at every automatic trigger, so it warns and
/// the phase is untouched.
///
/// ⚠️ **It is NOT re-entrant.** `File::try_lock` is `flock`-shaped: a second
/// `open` + lock in the SAME process blocks on itself. So no holder may call
/// another — `phase_start`'s reap loop calls `phase_reap` per phase, each
/// taking and dropping the lock, and never wraps the loop in one.
///
/// The lock alone does not close the race — see `phase_rehydrate` and
/// `phase_reap`, which both re-read `state.json` under it. Serializing two
/// launches without that just makes them consecutive rather than simultaneous.
fn acquire_run_lock(run_name: &str) -> io::Result<File> {
    let path = run_dir(run_name).join("run.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    match lock.try_lock() {
        Ok(()) => Ok(lock),
        Err(TryLockError::WouldBlock) => Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            format!(
                "another drovr command is moving run '{run_name}'s panes around (a \
                 rehydrate may be waiting up to 30s for its agent to come up). Wait for \
                 it to finish and look at the phase again — two rehydrates of one phase \
                 would leave two live agents and record only one, and reaping a phase a \
                 rehydrate is bringing back would close the pane it just opened."
            ),
        )),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// Give back a pane whose agent is live but **misattributed** — the phase
/// records it, and the phase's `pane_agent` describes a different conversation.
///
/// This is the resume path's failure, and only the resume path's. A resume
/// deliberately does NOT `record_launch`: the conversation is meant to be the
/// same one, so the agent record must survive the relaunch. When the resume is
/// then not confirmed, that record is describing a stranger — the phase claims
/// a pane running an agent it cannot account for, and `HoldsPane` refuses every
/// retry for as long as it stands.
///
/// # ⭐ The rule: POSITIVE EVIDENCE of misattribution, never mere doubt
///
/// The one caller is `ResumeEvidence::Contradicted`, where drovr **observed a
/// different session**. That is evidence the record does not describe the pane,
/// and it is what authorises destroying an agent.
///
/// Three neighbouring failures deliberately do NOT call this:
///
/// * **`ResumeEvidence::Unobserved`** — the agent came up and herdr never
///   reported which session it is in. Seeing nothing is not seeing a
///   contradiction: the id can surface a moment after the agent does (see
///   `a_session_that_shows_up_a_poll_late_is_still_a_confirmed_resume`), so
///   this is the same "I don't know" as the next bullet and takes the same
///   branch. Expressing the rule in terms of outcome VARIANTS rather than
///   evidence is how this arm was missed once already — hence
///   [`ResumeEvidence`] being an exhaustive enum.
///
/// * **`NeverReady { resuming: true }`** — drovr observed *nothing*. This
///   branch is built on "the marker is the evidence; absence of confirmation is
///   not evidence", and "I don't know" must not authorise closing a pane. The
///   usual cause is the recoverable one: the agent is parked on a first-run or
///   permission prompt and a human clicks through. drovr already treats that as
///   human-recoverable (`phase wait` exit 4, `diagnose_stuck_phase`), and
///   closing the pane throws it away irreversibly. The cost is that
///   `HoldsPane` refuses a retry while it stands — right while the agent is
///   starting, wrong once it has exited, and the note says so.
/// * **the reseed path** — `record_launch` ran, so the record DOES describe the
///   fresh agent; the note tells the operator to `phase send` it, and closing
///   it would destroy the very thing they were pointed at.
///
/// `mark_reaped` is the right transition and not a borrowing of reaping's: it
/// says "drovr closed this phase's pane", which is exactly what happened, and
/// it drops the registration in the same statement so the phase cannot claim a
/// pane that is gone. Retire → save → close, the same order and for the same
/// reason as [`surrender_unrecordable_pane`] — and the REVERSE of
/// [`phase_reap`]'s, which closes first; see there for why the two differ.
///
/// The phase's `pane_agent` is left ALONE, which is the point: the session id
/// this rehydrate failed to confirm is the one a retry needs.
/// Returns `Err` when the phase could NOT be released. That is not a warning to
/// print past: while the registration stands, `rehydratable` answers
/// `HoldsPane` for a pane that no longer exists, so **no outcome whose guidance
/// assumes a retry may be reported**. The caller must turn this into an error
/// that says what actually clears it.
fn surrender_misattributed_pane<H: Herdr>(
    h: &H,
    run: &mut RunState,
    phase: &str,
    pane: &str,
) -> io::Result<()> {
    let released = release_phase_from_pane(run, phase, pane)
        // Wrapped HERE rather than at the call site, so no caller can forget:
        // the `Err` this returns already says what is true and what clears it.
        .map_err(|e| unreleased_pane_error(&run.name, phase, pane, &e));
    // Closed either way. If the release landed this is the ordinary case; if it
    // did not, leaving the pane open would add an IMMORTAL pane (cleanup closes
    // only panes it can prove are drovr's, and an unretired one it cannot) to
    // an already-stuck registration — two manual repairs instead of one.
    if let Err(e) = h.pane_close(pane) {
        let cleanup = if released.is_ok() {
            "it is recorded as drovr's, so `drovr cleanup` will take it"
        } else {
            // Do not claim a retirement that never landed.
            "and the retirement did not land either, so `drovr cleanup` will treat it as \
             yours and leave it: close it by hand"
        };
        eprintln!(
            "drovr: warning: could not close pane {pane} after an unconfirmed resume ({e}); \
             {cleanup}"
        );
    }
    released
}

/// Clear `phase`'s registration of `pane` and record the retirement, **on disk**.
///
/// Written onto a FRESH read, like [`surrender_unrecordable_pane`] and for the
/// same reason: by the time this runs, up to `SEND_READY_TIMEOUT` of polling
/// has happened and capture persists through a copy of its own, so the caller's
/// `RunState` is not the state to write back. On success the caller's copy is
/// replaced wholesale rather than mutated in parallel — one assignment, so the
/// two cannot drift.
///
/// `pane` is retired explicitly rather than taken from `mark_reaped`'s return:
/// the pane being given back is the one the caller opened and is closing, which
/// is the honest thing to record whatever the registration happens to say.
fn release_phase_from_pane(run: &mut RunState, phase: &str, pane: &str) -> io::Result<()> {
    let mut fresh = RunState::load(&run.name)?;
    if let Some(p) = fresh.find_phase_mut(phase) {
        p.mark_reaped();
    }
    fresh.retire_pane(pane);
    fresh.save()?;
    *run = fresh;
    Ok(())
}

/// The refusal for a phase drovr closed a pane for but could not un-register.
///
/// Its whole job is to not repeat the lie this replaced: the phase is NOT
/// unchanged, a retry will NOT work, and making the run dir writable again does
/// not fix it by itself — nothing re-attempts the release. So it names the one
/// edit that does clear it, the same way [`missing_project_dir_error`] names
/// the field it needs.
fn unreleased_pane_error(run_name: &str, phase: &str, pane: &str, cause: &io::Error) -> io::Error {
    io::Error::new(
        cause.kind(),
        format!(
            "the agent for phase '{run_name}/{phase}' came up in a DIFFERENT conversation, so \
             drovr closed pane {pane} again — but the phase could not be released from it \
             ({cause}). It still records that pane, so rehydrate will now refuse with \"still \
             holds pane {pane}\", and nothing re-attempts the release on its own: fix the \
             write error, then run `drovr phase reap {q_run} {q_phase}`, which clears a \
             registration whose pane has already gone.",
            q_run = shell_single_quote(run_name),
            q_phase = shell_single_quote(phase),
        ),
    )
}

/// The prompt a reseeded agent gets. It says the conversation is gone, because
/// an agent that believes it is continuing its own work will confidently invent
/// the parts it cannot remember.
fn reseed_text(run_name: &str, phase: &str, seed: &str) -> String {
    format!(
        "You are phase '{phase}' of drovr run '{run_name}'. A previous agent held this \
         phase and its session could not be restored, so you are starting from the written \
         record rather than continuing a conversation: read {seed} and take it from there. \
         Anything the previous agent did not write down is gone — re-derive it from the \
         repository and the run directory instead of assuming it."
    )
}

/// Bring a phase whose pane is gone back on a fresh tab, RESUMING its recorded
/// agent session where one exists.
///
/// This is the inverse of reaping, and it deliberately lands before any reaping
/// does: closing panes is only safe once bringing them back provably works.
///
/// The shape of the recovery, in order:
///
/// 1. **Refuse a phase that still holds a pane.** "Has a pane" is the single
///    rule, on both this path and the HTTP one — there is no second, herdr-level
///    liveness check that could disagree with it. A phase drovr still records a
///    pane for is one to `drovr attach` to, not to duplicate an agent into.
///    (Cost of the simple rule: a phase whose pane herdr has lost, but which
///    drovr still records, cannot be rehydrated until something clears the
///    registration. [`phase_reap`] is that something — it takes the
///    `PaneStanding::Gone` path and drops the registration, which is why it is a
///    command and not only an automatic trigger.)
/// 2. **Never append.** Unlike [`phase_start`], an unknown name is an error —
///    the HTTP caller is unauthenticated and `safe_component` is a filename
///    check, not an authorization one.
/// 3. **Compose from the phase's own record, not this process's world.** The
///    backend, the profile and the session are one bundle
///    ([`crate::run::ResumeTarget`]); `run.project_dir` is the cwd. A daemon's
///    environment must not decide where a session resolves.
/// 4. **Persist the new pass, then sweep the marker, then launch** — the order
///    [`phase_start`] uses and for the same reason: no failure may leave a
///    state where `phase_wait` reports a completion for a pass whose agent is
///    not running.
/// 5. **On no session (or a backend with no resume surface), relaunch and
///    re-seed.** The fallback is the common case for anything cursor or codex
///    ran, and it must work.
///
/// The phase's `status` is deliberately NOT changed. A rehydrate restores an
/// agent; it does not decide whether the phase's work is finished, and flipping
/// a `Done` phase back to `Running` would tell `first_incomplete` and
/// `RunState::live_agent_pane` that the whole run had moved backwards.
pub fn phase_rehydrate<H: Herdr>(
    h: &H,
    run: &mut RunState,
    phase: &str,
) -> io::Result<RehydrateOutcome> {
    phase_rehydrate_with_timeout(
        h,
        run,
        phase,
        SEND_READY_TIMEOUT,
        CONFIRM_FLOOR,
        POLL_INTERVAL,
    )
}

/// [`phase_rehydrate`] with an injectable readiness timeout + poll interval, so
/// a test can exercise the reseed path's not-ready branch without waiting out
/// the full production timeout. Mirrors [`phase_send_with_timeout`].
fn phase_rehydrate_with_timeout<H: Herdr>(
    h: &H,
    run: &mut RunState,
    phase: &str,
    ready_timeout: Duration,
    confirm_floor: Duration,
    poll_interval: Duration,
) -> io::Result<RehydrateOutcome> {
    require_phase_name(phase)?;
    // ⭐ Everything below reads `state.json`, decides on it, and then launches
    // an agent — a read-modify-write across a 30-second wait, in a process that
    // is not the only one that can be running it. Two rehydrates of one phase
    // both saw `pane_id == None`, both passed the refusal, and both launched;
    // whole-file last-write-wins then dropped one of two live agents from the
    // record entirely.
    //
    // The lock serializes them and the RE-READ under it is what makes that
    // enough: the loser has to see what the winner wrote, or being second is
    // no different from being simultaneous. Held for the whole function.
    let _lock = acquire_run_lock(&run.name)?;
    // The caller's copy may predate the lock by any amount — the driver holds
    // one across a whole run, and the winner of the race above wrote while this
    // process was blocked. `state.json` under the lock is the only authority.
    *run = RunState::load(&run.name)?;
    // ONE precondition, shared with the HTTP handler and the agent tree — see
    // `RunState::rehydratable`. Written as separate checks it drifted twice:
    // first the handler checked two of the CLI's three, then the tree's
    // predicate turned out to omit the run-level prerequisites this function
    // enforced on its own.
    if let Err(why) = run.rehydratable(phase) {
        let quoted_run = shell_single_quote(&run.name);
        let quoted_phase = shell_single_quote(phase);
        return Err(match why {
            NotRehydratable::NoSuchPhase => io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "run '{}' has no phase '{phase}' — rehydrate never creates one \
                     (use `drovr phase start` for that)",
                    run.name
                ),
            ),
            NotRehydratable::Reviewer => io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "phase '{}/{phase}' is a review-panel agent, and a reviewer cannot be \
                     brought back able to do its job: it delivers findings through drovr's \
                     MCP server, which is handed over on the command line at launch and \
                     cannot be re-attached to a resumed session. Run the panel again \
                     instead: drovr code-review run {quoted_run} <task>",
                    run.name
                ),
            ),
            NotRehydratable::NoProjectDir => crate::phase::missing_project_dir_error(&run.name),
            NotRehydratable::NoWorkspace => io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "run '{}' has no herdr workspace (creation failed at `drovr new`); \
                     please recreate the run with `drovr new`",
                    run.name
                ),
            ),
            NotRehydratable::HoldsPane(pane) => io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "phase '{}/{phase}' still holds pane {pane}; rehydrate brings back a \
                     phase whose pane is gone. Look at that pane instead: herdr pane read \
                     {quoted_pane} (or herdr agent attach {quoted_pane}, if an agent is \
                     still attached to it) — or, if that pane is finished with, \
                     `drovr phase reap {quoted_run} {quoted_phase}` closes it and clears \
                     the registration this refusal is about",
                    run.name,
                    // `herdr pane read`, not `drovr attach <run>`: the latter
                    // resolves through `RunState::live_agent_pane`, which skips
                    // `Done` phases on purpose — and a `Done` phase is exactly
                    // what rehydrate is usually asked about, so it would attach
                    // to a DIFFERENT phase or deny any pane exists. And `read`
                    // rather than `agent attach`, because nothing clears
                    // `pane_id` when an agent merely EXITS — reaping is
                    // triggered by supersession, not by an agent going away —
                    // so this pane may well have no agent on it.
                    quoted_pane = shell_single_quote(&pane),
                ),
            ),
            NotRehydratable::NeverStarted => io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "phase '{}/{phase}' has never run, so there is nothing to bring back. \
                     Start it: drovr phase start {quoted_run} {quoted_phase}",
                    run.name
                ),
            ),
            NotRehydratable::NoAgentEverRan => io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "phase '{}/{phase}' has no agent on record — its last `phase start` \
                     never got an agent running, so there is no session, no backend and no \
                     seed to restore. Start it again (with its seed, which a rehydrate \
                     cannot recover): drovr phase start {quoted_run} {quoted_phase}",
                    run.name
                ),
            ),
        });
    }
    // `project_dir` and `workspace` are NOT re-checked here: `rehydratable`
    // above is the authority on both, so a second test would be a second place
    // for the answer to live.
    //
    // `readonly` asks `is_reviewer_phase_name` — the SAME predicate
    // `rehydratable` refuses reviewers on, not `review_phases` membership
    // beside it. Today it is always false (a reviewer never reaches this line),
    // but two spellings of "is this a reviewer" is exactly the drift the last
    // two rounds each fixed once: a `review_phases` entry whose name lacked the
    // prefix would pass the gate and then launch WITH `readonly_flag`, and a
    // reviewer-shaped name in `phases` the other way round. It is derived
    // rather than hardcoded to `false` because the day reviewers become
    // rehydratable again, the alternative is a resumed reviewer silently
    // launched as a second WRITER in a run whose whole discipline is that there
    // is exactly one.
    let readonly = is_reviewer_phase_name(phase);
    let seed = run
        .find_phase(phase)
        .expect("rehydratable() proved the phase is in this run")
        .handoff_doc
        .clone();
    // Compose while the borrow of `run` is still alive, so the resume bundle
    // never has to be taken apart into loose owned values to escape it. The
    // block ends before anything mutates.
    //
    // `resumable_for(backend)` is the single chokepoint task 2 spent three
    // rounds building, and `ResumeTarget` is how its proof travels: session,
    // backend AND profile, or nothing. `Config::resume_launch` takes the whole
    // target and reads the backend out of it itself, so this call site — the
    // one that composes `--resume` — has no way to pair an id with the wrong
    // agent.
    let cwd = run.project_dir.clone();
    let cfg = load_config()?;
    let (launch, profile, resume_session, fresh_backend) = {
        let existing = run
            .find_phase(phase)
            .expect("the phase was located above and nothing removed it");
        // What a plain relaunch uses: the phase's own launch record where it
        // has one — a phase keeps its backend and profile even when its
        // conversation is unrecoverable — else the run's configured backend.
        let recorded = existing.pane_agent();
        let fresh_backend = recorded
            .map(|a| a.backend().to_owned())
            .unwrap_or_else(|| run.agent.clone().unwrap_or_else(|| "claude".to_string()));
        // The RECORDED profile or nothing — deliberately NOT falling back to
        // `agent_profile_env()`. This process may be the review server, a
        // long-lived daemon whose `CLAUDE_CONFIG_DIR` has nothing to do with
        // the account this run's agents authenticate as. `None` means the
        // default profile, which is what a phase recording no profile was
        // launched under — an honest unknown, not a guess.
        let fresh_profile = recorded.and_then(|a| a.profile().map(str::to_owned));

        let resumed = match existing.resume_target() {
            Some(target) => cfg.resume_launch(&target, &cwd, readonly)?.map(|launch| {
                (
                    launch,
                    target.profile().map(str::to_owned),
                    // Carried forward because it is what the resume has to be
                    // CHECKED against later: the id drovr asked for, kept
                    // before any poll can overwrite the phase's record with
                    // whatever session the new pane turns out to hold.
                    target.session().clone(),
                )
            }),
            None => None,
        };
        // `Some(session)` is a fact about the composed COMMAND, not about
        // whether a session existed: a backend with no resume surface has a
        // perfectly good session id and still cannot be told to use it.
        match resumed {
            Some((launch, profile, session)) => (launch, profile, Some(session), fresh_backend),
            None => (
                cfg.launch(&fresh_backend, &cwd, readonly, None)?,
                fresh_profile,
                None,
                fresh_backend,
            ),
        }
    };

    // ⚠️ NOTHING IS MUTATED UNTIL THE LAUNCH SUCCEEDS, and that is the opposite
    // of `phase_start`'s persist-first order on purpose.
    //
    // `phase_start` re-enters a phase to do NEW work, so losing the previous
    // pass's completion is intended and persist-then-sweep is right. A
    // rehydrate is RECOVERY: the phase's work may already be finished, and
    // task 1 established that the `<phase>.done` MARKER is the evidence for
    // that — a stale `Done` status is explicitly not accepted as proof. So
    // sweeping the marker (or minting a new pass, which makes the old marker's
    // token mismatch) and THEN failing to relaunch leaves a phase that is
    // neither complete-provable nor running: the next `phase wait` blocks
    // forever, and the only way out is hand-editing the run dir.
    //
    // Every failure below therefore leaves the phase exactly as it was ON DISK
    // — still reaped, still `Done`, still holding its marker and the pass that
    // marker was stamped with — so a retry starts from the same place.
    //
    // ⚠️ On disk, not in memory: if `run.save()` below fails, the in-memory
    // `RunState` has already been mutated while `state.json` (written
    // tmp+rename) has not, so the caller's copy is left ahead of disk. That is
    // now harmless from both ends — this function re-reads `state.json` under
    // its lock before it decides anything, so a reused `RunState` cannot carry
    // a stale claim into the next call, and `surrender_unrecordable_pane`
    // deliberately writes onto a fresh read rather than onto that copy.
    let pass = new_pass_token();

    // A fresh tab, never `run.root_pane` — a rehydrated phase must be as
    // independently closeable as a started one, and the root shell anchors the
    // workspace for the run's whole lifetime.
    let ws = run
        .workspace
        .clone()
        .expect("rehydratable() proved the run has a workspace");
    let pane = h.tab_create(&ws, phase, &cwd)?;
    // `pane_run` (via `launch_in_pane`), NOT `herdr agent start <pane>`, which
    // was raised as the likelier fit and checked against the real CLI:
    // `herdr agent start <NAME> --kind <KIND> --pane <ID> [-- <AGENT_ARG>…]`
    // *can* carry `--resume <id>` in its trailing args, but it has **no env
    // option** — and `DROVR_PASS` (what makes the marker this agent drops
    // attributable to this pass) and `CLAUDE_CONFIG_DIR` (what decides where the
    // session resolves) both have to be in the agent's environment. Its `--kind`
    // is a closed list too, while drovr's agent map takes an arbitrary command.
    // So a rehydrated pane is launched exactly the way a started one is.
    if let Err(e) = launch_in_pane(
        h,
        &run.name,
        phase,
        &pane,
        launch.command(),
        &pass,
        profile.as_deref(),
    ) {
        // The tab is always ours (`tab_create` is one line up), so there is no
        // "did this call create it" question — see `discard_unlaunched_pane`
        // for why a pane drovr opened and never recorded is worse than a leak.
        // The PHASE is untouched: no new pass, no cleared marker, no replaced
        // agent record.
        discard_unlaunched_pane(h, run, &pane);
        return Err(e);
    }

    // The agent is running. Only now does any of this become true.
    {
        let p = run
            .find_phase_mut(phase)
            .expect("the phase was located above and nothing removed it");
        p.pass = Some(pass.clone());
        if resume_session.is_none() {
            // A relaunch REPLACES the agent record, which is the only way a
            // session is ever discarded: a new agent process means the id the
            // old record names is no longer this phase's conversation.
            p.record_launch(&fresh_backend, profile.clone());
        }
        // Clears `reaped` in the same statement: a phase with a live pane is
        // not a reaped one.
        p.set_pane(pane.clone());
    }
    // Save BEFORE sweeping the marker — task 1's rule, and it still applies now
    // that both happen late: if the save fails, the marker survives alongside
    // the pass it was stamped with, so `phase_wait` can still complete off it.
    // Sweeping first and then failing to save is the hole that leaves a phase
    // `Done` with its evidence gone.
    if let Err(e) = run.save() {
        // ⚠️ An agent is LIVE in `pane` and `state.json` does not name it.
        // That is the immortal-pane bug: main's `8173f03` made `drovr cleanup`
        // close only panes it can PROVE are drovr's, so a live tab nothing
        // records reads as the human's — never closed, and it blocks
        // `workspace_close` for the whole run. "Never live-but-unrecorded" is
        // the invariant, and when the record cannot be written the only way to
        // keep it is to not leave the pane live.
        surrender_unrecordable_pane(h, run, &pane);
        return Err(io::Error::new(
            e.kind(),
            format!(
                "phase '{}/{phase}' was relaunched, but its pane could not be recorded \
                 ({e}), so the agent was closed again rather than left running with \
                 nothing tracking it. The phase is untouched on disk — retry once the \
                 run directory is writable.",
                run.name
            ),
        ));
    }
    // Best-effort, and NOT `?`. By this line the agent is running and its pane
    // is durably recorded, so a hard error here would report a rehydrate that
    // fully succeeded as a failure — the CLI would exit 1 and the HTTP handler
    // would answer 500 "nothing happened" about a live pane, and the retry that
    // invites is then refused with `HoldsPane`, sending the operator to look at
    // a pane they were just told did not exist. That is the same
    // reports-the-wrong-thing class as the bug this ordering fixed, inverted.
    //
    // Safe to swallow because the marker is already INERT: the new pass is on
    // disk, and `marker_completes_pass` rejects a token that does not match it.
    // Sweeping is defence in depth here, so its failure is a warning, not an
    // outcome. (In `phase_start` the same call IS fatal — there nothing else
    // invalidates the marker at that point.)
    if let Err(e) = remove_stale_marker(&run.name, phase) {
        eprintln!(
            "drovr: warning: could not remove the stale completion marker for \
             '{}/{phase}' ({e}). The phase is rehydrated and the marker is inert \
             (its token no longer matches this pass), so `phase wait` is correct \
             either way — but the file is still there.",
            run.name
        );
    }

    // ⚠️ The readiness gate is NOT the reseed path's alone, and gating only that
    // one was a real defect. `pane_run` returning `Ok` means the shell command
    // was *issued*, nothing more. A resume whose recorded id no longer resolves
    // (the session file pruned, the profile's storage cleared, the backend's id
    // format changed) launches, fails to find the conversation, and errors out or
    // parks. Reporting `Resumed` there would claim "same conversation, same
    // agent" on the strength of a spawn, and hand a driver an exit 0 for an
    // agent that was never resumed.
    //
    // It also re-captures the session on the way past (`poll_phase_pane`), so a
    // rehydrated phase is immediately rehydratable again.
    //
    // ⚠️⚠️ And readiness is only the FIRST of the two things a resume has to
    // prove. See `confirm_resumed_session` below: "an agent is up" and "your
    // conversation is back" are different claims, and a stale id produces the
    // first while quietly failing the second.
    // ONE deadline across both waits. Confirmation is not a second timeout
    // bolted on: a resume that comes up instantly and then never reports its
    // session must not be able to hold the caller for twice `ready_timeout`.
    let polling = Polling::until(Instant::now() + ready_timeout, poll_interval);
    let Some(ready) = wait_agent_ready_until(h, run, phase, polling) else {
        // ⚠️ THE PANE IS KEPT HERE, on every path, and the asymmetry with
        // `ResumeEvidence::Contradicted` below is deliberate.
        //
        // `NeverReady` means drovr observed NOTHING — no status at all. This
        // branch is built on "the marker is the evidence; absence of
        // confirmation is not evidence", and destroying a pane is not something
        // "I don't know" may authorise. The common cause is precisely the
        // recoverable one: the agent is parked on a first-run or permission
        // prompt, a human clicks through, and the conversation is fine. drovr
        // already treats that as human-recoverable — `phase wait` has exit 4
        // and `diagnose_stuck_phase` for it — and closing the pane throws that
        // away irreversibly.
        //
        // The cost, and it is real: while that pane stands, `rehydratable`
        // answers `HoldsPane` and refuses a retry. That is right while the
        // agent is starting up and wrong once it has exited, and drovr cannot
        // tell those apart without a liveness probe this predicate
        // deliberately does not do. The note below therefore sends the operator
        // to the pane rather than promising a retry.
        return Ok(RehydrateOutcome::Incomplete(Unfinished::NeverReady {
            pane,
            waited: ready_timeout,
            resuming: resume_session.is_some(),
            had_seed: seed.is_some(),
        }));
    };
    // ⚠️ AND "the agent is up" is still not "your session came back". A stale id
    // makes the backend start a FRESH conversation — attached, idle, and
    // indistinguishable from a successful resume unless you ask herdr WHICH
    // session it is in. The ⟳ promises the actual conversation, so `Resumed` is
    // claimed on that answer and on nothing weaker.
    if let Some(expected) = resume_session {
        return Ok(
            match confirm_resumed_session(
                h,
                run,
                phase,
                // The backend the COMMAND was composed for, out of the launch
                // itself — never a name paired up beside it. That is the whole
                // point of `AgentLaunch` carrying both.
                launch.backend(),
                &expected,
                Some(ready),
                // Whatever is left of the shared budget, but never less than
                // the floor: a launch that spent it all must not get to decide
                // whether the conversation came back. See `CONFIRM_FLOOR`.
                polling.with_floor(confirm_floor),
            ) {
                // ⭐ THE RULE, AT THE POINT THE DECISION IS MADE: a pane is
                // surrendered only on evidence that it is running a DIFFERENT
                // conversation. An exhaustive match over
                // [`ResumeEvidence`] rather than a test on an `Option`, because
                // "and what if we saw nothing" is exactly the arm that got
                // missed when this was expressed as "not confirmed".
                ResumeEvidence::Confirmed => RehydrateOutcome::Resumed,
                ResumeEvidence::Contradicted(observed) => {
                    // The phase is recording a pane running someone else's
                    // conversation — see `surrender_misattributed_pane`.
                    //
                    // ⚠️ The `?` is load-bearing. A release that did not land
                    // leaves `HoldsPane` answering for a pane that is gone,
                    // while this outcome's note tells the operator the phase is
                    // unchanged and a retry will work. Reporting that over a
                    // stuck registration is guidance that can only mislead, so
                    // the failure becomes an error that says what actually
                    // clears it. (`io::Result` is `#[must_use]` and clippy runs
                    // at `-D warnings`, so this cannot be quietly dropped back
                    // to a warning.)
                    surrender_misattributed_pane(h, run, phase, &pane)?;
                    RehydrateOutcome::Incomplete(Unfinished::ResumeContradicted {
                        pane,
                        expected,
                        observed,
                    })
                }
                // NOTHING was observed, so nothing is destroyed — the same
                // branch `NeverReady { resuming: true }` takes, for the same
                // reason. The pane stays and the note sends the operator to it.
                ResumeEvidence::Unobserved => {
                    RehydrateOutcome::Incomplete(Unfinished::ResumeUnobserved { pane, expected })
                }
            },
        );
    }
    let Some(seed) = seed else {
        return Ok(RehydrateOutcome::Incomplete(Unfinished::NoSeed { pane }));
    };
    match h.agent_send(&pane, &reseed_text(&run.name, phase, &seed)) {
        Ok(()) => Ok(RehydrateOutcome::Reseeded),
        Err(e) => Ok(RehydrateOutcome::Incomplete(Unfinished::SeedUndelivered {
            pane,
            seed,
            error: e.to_string(),
        })),
    }
}

// ---------------------------------------------------------------------------
// Reap
// ---------------------------------------------------------------------------

/// What drovr ESTABLISHED about a phase's pane, before deciding whether it may
/// drop the phase's registration of it.
///
/// Three states and not a `bool`, because two of them arrive through the same
/// `None` from [`Herdr::pane_info`] and they authorise opposite actions. This is
/// the fourth time on this branch that a two-state encoding of a three-state
/// fact would have produced a bug (`Capture`'s fill-vs-replace, reviewer
/// identity by list-vs-name, `ResumeEvidence`'s confirmed/contradicted/
/// unobserved), so the states are named.
///
/// The opposite actions, precisely:
///
/// * [`PaneStanding::Gone`] — herdr says there is no such pane. Clearing the
///   registration is then the WHOLE POINT: it is the supported repair for a
///   phase whose pane herdr has lost, which is otherwise stuck answering
///   `HoldsPane` to every rehydrate with nothing able to clear it.
/// * [`PaneStanding::Unknown`] — herdr could not be asked. Clearing here drops a
///   registration for a pane that may be perfectly alive, which strands it:
///   `drovr cleanup` closes only panes it can prove are drovr's, so an
///   unrecorded one reads as the human's and is never closed, while it blocks
///   `workspace_close` for the whole run. "I don't know" authorises nothing —
///   the same rule the resume path settled on.
#[derive(Debug, PartialEq, Eq)]
enum PaneStanding {
    /// herdr answered `pane.get`: the pane is there, and can be closed.
    ///
    /// It says NOTHING about whether an agent is still attached, and reaping
    /// deliberately does not ask. A pipeline phase's `claude` does not exit when
    /// it runs `drovr phase done` — it sits at its composer — so "no agent
    /// attached" is not a state a finished phase reaches, and waiting for it
    /// would mean never reaping anything. Supersession is the evidence that a
    /// phase is finished with; the agent's own exit is not.
    Live,
    /// herdr answered that there is NO SUCH PANE.
    Gone,
    /// herdr could not be asked — unreachable daemon, socket error, a response
    /// shape drovr could not parse. Nothing was established.
    Unknown,
}

/// Classify a phase's pane from one poll, disambiguating [`Herdr::pane_info`]'s
/// `None`.
///
/// `pane_info` returns `None` both for a pane that is gone and for a poll that
/// merely failed — [`PaneInfo`]'s doc says so explicitly, and says reaping is
/// the thing that turns on the distinction. [`Herdr::pane_exists`] is the
/// disambiguator, and it is biased the right way for this: only herdr's explicit
/// `pane_not_found` answers `false`, so an unreachable daemon lands in
/// [`PaneStanding::Unknown`] rather than being read as proof of death.
///
/// The second herdr round trip happens ONLY on the `None` path, which is the
/// path that has already failed.
fn pane_standing<H: Herdr>(h: &H, poll: Option<&PaneInfo>, pane: &str) -> PaneStanding {
    match poll {
        Some(_) => PaneStanding::Live,
        None if !h.pane_exists(pane) => PaneStanding::Gone,
        None => PaneStanding::Unknown,
    }
}

/// What a [`phase_reap`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum ReapOutcome {
    /// The pane was closed and the phase released from it.
    Closed { pane: String },
    /// The pane was already gone, and the phase's registration of it has been
    /// cleared. This is the repair for a stuck `HoldsPane`.
    Cleared { pane: String },
    /// The phase holds no pane at all — already reaped, never launched, or its
    /// pane died with its workspace. Nothing was done and nothing was wrong,
    /// which is what makes a second reap of one phase emit no close.
    NothingToReap,
    /// The pane is still there and the phase still holds it. **The phase is
    /// EXACTLY as it was**: same status, not reaped, still recorded — which is
    /// what keeps the pane inside `drovr_pane_ids` so `drovr cleanup` can still
    /// prove it is drovr's.
    Kept { pane: String, why: PaneKept },
}

/// Why a reap left the pane where it was.
#[derive(Debug, PartialEq, Eq)]
pub enum PaneKept {
    /// `pane_close` failed, carrying herdr's reason.
    CloseFailed(String),
    /// herdr could not say whether the pane exists, so drovr cannot prove it is
    /// gone — see [`PaneStanding::Unknown`].
    Unreadable,
}

/// How the pane a phase records stopped existing.
///
/// A named pair rather than a `bool`, because [`unreaped_pane_error`] has to
/// describe two different events — drovr closed it a moment ago, or herdr had
/// already lost it — and the operator's next step differs.
#[derive(Debug, Clone, Copy)]
enum PaneGone {
    ClosedByThisReap,
    AlreadyMissing,
}

/// Close the pane a finished phase is holding, and release the phase from it.
///
/// **Reaping is supersession, not completion** — see `skills/pipeline`: `Done`
/// is not terminal for a pane, because the implement↔review loop re-enters the
/// same pane with `drovr phase send` and no `phase start`. So nothing here is
/// triggered by a phase finishing. The triggers are the moments the run has
/// provably moved past it: `phase_start` reaps every other finished phase after
/// its own launch succeeds, `code_review_run` reaps its panel after the findings
/// are merged, and `drovr phase reap` is the operator saying so directly.
///
/// # Order, and why it is the reverse of the surrender paths
///
/// [`surrender_misattributed_pane`] records the retirement and THEN closes, so
/// a failed close still leaves the pane provably drovr's. Reaping closes FIRST
/// and releases only if that worked, and the difference is what a failure has to
/// leave behind:
///
/// * a surrender has already decided the pane must not stay — the record is
///   wrong either way, so it is written first;
/// * a reap is **best-effort and must never fail the phase**. A close that fails
///   means the pane is still there, so the honest record is the one that was
///   already on disk. Clearing `pane_id` anyway would be the immortal-pane bug
///   arriving by the front door: `drovr cleanup` closes only panes it can prove
///   are drovr's (main's `8173f03`), and a live pane no phase records reads as
///   the human's — never closed, and blocking `workspace_close` for the run.
///
/// The release itself is [`release_phase_from_pane`], shared with the surrender
/// path rather than written a second time: retire → mark reaped → save, all
/// three onto a FRESH read, in one save that cannot half-land. That ordering is
/// the family's lesson (`discard_unlaunched_pane`, `surrender_unrecordable_pane`,
/// `surrender_misattributed_pane`), and reusing it is how this stays the fourth
/// member rather than becoming a fifth.
///
/// # `pane_close`, never `tab_close`
///
/// A phase occupies one pane, in a tab drovr created for it — but the human can
/// split their own pane into that tab (a shell for the tests beside the agent),
/// and `tab.close` takes every pane in the tab. `8173f03` established "never
/// close what you cannot prove is yours" at PANE granularity, and closing the
/// tab would quietly widen that.
///
/// The reason this costs nothing: **verified live against herdr 0.7.5 — closing
/// the last pane in a tab destroys the tab.** (Probe: `tab create` → `pane
/// split` → `pane close` the split → `pane close` the original → `tab get`
/// answers `tab_not_found`.) So in the ordinary case, where drovr's pane is the
/// tab's only pane, the tab disappears exactly as `tab_close` would have made it
/// — and in the case where it is not, the human's pane and its tab survive. It
/// is also the primitive `drovr cleanup` and all three disposal paths already
/// use, so there is one teardown call in drovr rather than two.
///
/// # Best-effort, and the one thing that is not
///
/// Every herdr call here may fail without failing the caller: a failed poll, a
/// failed close and a failed focus restore all end in [`ReapOutcome::Kept`] or
/// are ignored. The single `Err` is a release that could not be SAVED after the
/// pane is already gone — see [`unreaped_pane_error`], which names what clears
/// it. The automatic triggers treat even that as a warning; `drovr phase reap`
/// surfaces it, because the operator asked.
pub fn phase_reap<H: Herdr>(h: &H, run: &mut RunState, phase: &str) -> io::Result<ReapOutcome> {
    require_phase_name(phase)?;
    // Same lock and same re-read as `phase_rehydrate`, and for the same reason
    // in reverse: reaping a phase a rehydrate is bringing back would close the
    // pane it just opened, and the driver's `RunState` may be minutes old — a
    // reap decided on a stale copy closes whatever pane that copy remembers.
    let _lock = acquire_run_lock(&run.name)?;
    *run = RunState::load(&run.name)?;
    let pane = match run.reapable(phase) {
        Ok(pane) => pane.to_owned(),
        // Not an error: idempotence is the point. A second reap of one phase,
        // and a reap of a phase that never held a pane, both land here.
        Err(NotReapable::NoPane) => return Ok(ReapOutcome::NothingToReap),
        Err(NotReapable::NoSuchPhase) => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "run '{}' has no phase '{phase}' — reap never creates one",
                    run.name
                ),
            ));
        }
        Err(NotReapable::RootShell(pane)) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "phase '{}/{phase}' is recorded as holding pane {pane}, which is run \
                     '{}'s root shell — herdr destroys a workspace when its last pane \
                     closes, so reaping it would take the workspace and every other \
                     phase in it. (Only a state.json written by an older drovr, where the \
                     first phase claimed the root pane, can look like this; the run is \
                     otherwise fine.) Use `drovr cleanup {}` to tear the whole run down.",
                    run.name,
                    run.name,
                    shell_single_quote(&run.name),
                ),
            ));
        }
    };

    // Poll BEFORE closing, and through `poll_phase_pane` rather than
    // `Herdr::pane_info`, because it captures the session on the way past — and
    // this is the LAST time anything will look at this pane. herdr reports
    // `agent_session` only while the agent is alive, so an id that has not been
    // banked by now is one a rehydrate will never have. Reaping without that is
    // a downgrade: the pane goes and nothing can bring the conversation back.
    let poll = poll_phase_pane(h, run, phase);
    match pane_standing(h, poll.as_ref(), &pane) {
        // Nothing was established, so nothing is destroyed and nothing is
        // dropped. The phase is untouched and a later reap will ask again.
        PaneStanding::Unknown => Ok(ReapOutcome::Kept {
            pane,
            why: PaneKept::Unreadable,
        }),
        // herdr has already lost the pane. There is nothing to close, and
        // clearing the registration is the entire job: this is the state that
        // leaves a phase answering `HoldsPane` to every rehydrate forever.
        PaneStanding::Gone => {
            release_phase_from_pane(run, phase, &pane).map_err(|e| {
                unreaped_pane_error(&run.name, phase, &pane, PaneGone::AlreadyMissing, &e)
            })?;
            Ok(ReapOutcome::Cleared { pane })
        }
        PaneStanding::Live => {
            // Focus is captured and restored around the close — see
            // `close_pane_preserving_focus`, shared with `reap_retired` so the
            // rule is not written twice.
            match close_pane_preserving_focus(h, &pane) {
                // ⚠️ The registration STAYS. See the ordering note above: a
                // pane that is still there and no longer recorded is immortal.
                Err(e) => Ok(ReapOutcome::Kept {
                    pane,
                    why: PaneKept::CloseFailed(e.to_string()),
                }),
                Ok(()) => {
                    release_phase_from_pane(run, phase, &pane).map_err(|e| {
                        unreaped_pane_error(&run.name, phase, &pane, PaneGone::ClosedByThisReap, &e)
                    })?;
                    Ok(ReapOutcome::Closed { pane })
                }
            }
        }
    }
}

/// The refusal for a phase whose pane is gone and which could not be released
/// from it.
///
/// The sibling of [`unreleased_pane_error`], and it has the same job: not to
/// repeat a comfortable lie. The phase now records a pane that does not exist,
/// so `drovr attach` and `phase send` aim at nothing, and rehydrate refuses with
/// "still holds pane" — and nothing re-attempts the release on its own.
///
/// Unlike its sibling it does NOT tell the operator to hand-edit `state.json`,
/// because the command that fixes this now exists: re-running the reap once the
/// run directory is writable takes the [`PaneStanding::Gone`] path and clears
/// exactly this.
fn unreaped_pane_error(
    run_name: &str,
    phase: &str,
    pane: &str,
    gone: PaneGone,
    cause: &io::Error,
) -> io::Error {
    let what = match gone {
        PaneGone::ClosedByThisReap => "drovr closed pane",
        PaneGone::AlreadyMissing => "herdr no longer has pane",
    };
    io::Error::new(
        cause.kind(),
        format!(
            "{what} {pane}, but phase '{run_name}/{phase}' could not be released from it \
             ({cause}). It still records that pane, so `drovr attach` and `drovr phase \
             send` will aim at a pane that is gone and rehydrate will refuse with \"still \
             holds pane {pane}\". Nothing clears that on its own: fix the write error, \
             then run `drovr phase reap {q_run} {q_phase}` again — it clears a \
             registration whose pane has already gone.",
            q_run = shell_single_quote(run_name),
            q_phase = shell_single_quote(phase),
        ),
    )
}

/// Close the panes this run RETIRED — the ones drovr made, that no phase points
/// at any more, and that nothing else will ever close before `drovr cleanup`.
///
/// # The leak this closes
///
/// [`phase_reap`] works per phase, through the phase's `pane_id`. A retired pane
/// has no phase pointing at it — that is what retirement MEANS — so no reap can
/// reach it. `code_review_run` makes one every time it replaces a reviewer: it
/// retires the predecessor's pane and drops the registration, and nothing then
/// closed it. Left alone, panels accumulate exactly the way this branch exists
/// to stop.
///
/// # Why closing one is safe
///
/// [`RunState::retired_panes`] is the record that a pane is DROVR'S after the
/// phase that held it let go — it exists so `drovr cleanup` can close it under
/// main's `8173f03` (never close what you cannot prove is yours). A pane in that
/// list is therefore provably not a human's, and closing it early is the same
/// act `drovr cleanup` would perform later. WHICH entries qualify is
/// [`RunState::reapable_retired`]'s rule, written once where it can be tested,
/// rather than a filter with a comment above the close.
///
/// A retired pane may still have a LIVE agent in it — a reviewer replaced
/// because it "produced nothing usable" was wedged, not dead — and closing it
/// ends that agent. That is the intent, not a side effect: the decision to
/// replace it was already taken, by the caller that retired it, and everything
/// downstream has been reading its replacement ever since. Reaping does not ask
/// whether an agent is attached for the same reason [`PaneStanding::Live`] says
/// nothing about it.
///
/// # What it does with a pane it cannot see
///
/// Classified by [`pane_standing`], the same three states a phase's pane gets,
/// because it is the same question — and "I don't know" authorises nothing:
///
/// * [`PaneStanding::Live`] → close it, and forget the retirement only if that
///   worked. A close that fails leaves the entry exactly where it is, because
///   the entry is the ONLY thing that still says the pane is drovr's; dropping
///   it would leave a live pane nothing records, which cleanup then reads as the
///   human's and never closes.
/// * [`PaneStanding::Gone`] → forget the retirement. There is nothing to close,
///   and see [`RunState::forget_retired_panes`] for why an entry with no pane
///   behind it is worse than no entry at all.
/// * [`PaneStanding::Unknown`] → nothing. It is probed again at the next
///   trigger.
///
/// The poll is `Herdr::pane_info`, NOT [`poll_phase_pane`] as [`phase_reap`]'s
/// is. That one polls through the phase in order to bank the agent's session on
/// the way past; here there is no phase to bank it against — the registration
/// was cleared before the pane was ever retired — so the capturing poll has
/// nothing to capture onto.
///
/// # Best-effort, with no `Err` at all
///
/// Returns `()`, unlike [`phase_reap`]: every caller is a trigger that has
/// already done its real work (a launch that succeeded, a panel whose verdict is
/// on disk, a reap the operator asked for), and this must never turn one of them
/// into a failure. A pane it could not close is a warning, and `drovr cleanup`
/// still reclaims it — which is exactly the state the run was in before this
/// existed.
///
/// Takes the run lock for the same reason [`phase_reap`] does: it is a
/// read-modify-write of the same file, racing a rehydrate in the other
/// direction. So it must never be called from inside a lock holder — the lock is
/// `flock`-shaped and not re-entrant. Every call site is outside
/// [`phase_reap`]'s, deliberately.
pub fn reap_retired<H: Herdr>(h: &H, run: &mut RunState) {
    // A read WITHOUT the lock, whose only job is to decide whether there is any
    // work at all — never what the work is; the authoritative read is the one
    // under the lock below. It is here so the overwhelming majority of launches,
    // which have retired nothing, neither take a lock nor warn about one a
    // concurrent rehydrate is holding. An unreadable state.json answers "no
    // work", the same thing every other unestablished fact here answers.
    let worth_locking = RunState::load(&run.name).is_ok_and(|s| !s.reapable_retired().is_empty());
    if !worth_locking {
        return;
    }
    let _lock = match acquire_run_lock(&run.name) {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!(
                "drovr: warning: could not sweep run '{}'s retired panes ({e}); \
                 `drovr cleanup` will reclaim them",
                run.name
            );
            return;
        }
    };
    // Re-read under the lock, like `phase_reap`: a sweep decided on the caller's
    // copy closes whatever panes that copy remembers, and a driver holds one
    // `RunState` for a whole run while panels retire panes from another process.
    let fresh = match RunState::load(&run.name) {
        Ok(fresh) => fresh,
        Err(e) => {
            eprintln!(
                "drovr: warning: could not read run '{}' to sweep its retired panes ({e})",
                run.name
            );
            return;
        }
    };
    let targets = fresh.reapable_retired();
    if targets.is_empty() {
        return;
    }
    *run = fresh;

    let mut gone: Vec<String> = Vec::new();
    for pane in targets {
        match pane_standing(h, h.pane_info(&pane).as_ref(), &pane) {
            PaneStanding::Unknown => {}
            PaneStanding::Gone => gone.push(pane),
            PaneStanding::Live => match close_pane_preserving_focus(h, &pane) {
                // Said out loud: a pane vanishing from the user's herdr is a
                // visible event, and it belongs to no phase they could look up.
                Ok(()) => {
                    eprintln!("drovr: closed retired pane {pane} of run '{}'", run.name);
                    gone.push(pane);
                }
                Err(e) => eprintln!(
                    "drovr: warning: left retired pane {pane} open ({e}); \
                     `drovr cleanup` will reclaim it"
                ),
            },
        }
    }
    if gone.is_empty() {
        return;
    }
    // One save onto a FRESH read, the same shape as `release_phase_from_pane`:
    // the polls and closes above take herdr round trips, and the caller's copy
    // is not the state to write back. A save that fails changes nothing — the
    // entries stay, and the next sweep re-establishes that they are gone and
    // tries again.
    match RunState::load(&run.name) {
        Ok(mut fresh) => {
            fresh.forget_retired_panes(&gone);
            match fresh.save() {
                Ok(()) => *run = fresh,
                Err(e) => eprintln!(
                    "drovr: warning: closed {} retired pane(s) of run '{}' but could not \
                     record it ({e}); the next sweep will notice they are gone",
                    gone.len(),
                    run.name
                ),
            }
        }
        Err(e) => eprintln!(
            "drovr: warning: could not record the sweep of run '{}'s retired panes ({e})",
            run.name
        ),
    }
}

/// Close one pane and put the user's view back where it was.
///
/// Closing a pane makes herdr reassign focus, and drovr must not move the user's
/// view as a side effect of its own bookkeeping — the same reason
/// [`launch_in_pane`] captures and restores it. Restoring is best-effort: a
/// focus that could not be put back is no reason to report a close that happened
/// as one that did not, so what comes back is `pane_close`'s own result.
///
/// One function because [`phase_reap`] and [`reap_retired`] close panes for the
/// same reason, and a focus rule written twice is a focus rule that drifts.
fn close_pane_preserving_focus<H: Herdr>(h: &H, pane: &str) -> io::Result<()> {
    let prev_focus = h.focused_workspace();
    let closed = h.pane_close(pane);
    if let Some(prev) = prev_focus {
        let _ = h.workspace_focus(&prev);
    }
    closed
}

/// Reap every phase a launch of `starting` has superseded.
///
/// **Best-effort at every step, and that is a hard requirement**: this runs
/// after `phase_start` has already launched an agent and recorded it, so a
/// failure here must never turn a started phase into a failed command. Every
/// outcome is reported and none is propagated.
///
/// WHICH phases is [`RunState::superseded_by`]'s rule, not a filter written
/// here — the condition belongs in the API that answers it, or the automatic
/// trigger and the reap itself get to disagree about what is reapable.
///
/// The lock is taken and dropped per phase, inside [`phase_reap`]: the run lock
/// is `flock`-shaped and not re-entrant, so wrapping this loop in one would
/// deadlock against the first `phase_reap` it calls.
fn reap_superseded<H: Herdr>(h: &H, run: &mut RunState, starting: &str) {
    for name in run.superseded_by(starting) {
        match phase_reap(h, run, &name) {
            // Said out loud: a pane vanishing from the user's herdr is a visible
            // event, and a driver reading drovr's output should be able to see
            // which phase it belonged to.
            Ok(ReapOutcome::Closed { pane }) => {
                eprintln!("drovr: closed pane {pane} of finished phase '{name}'");
            }
            Ok(ReapOutcome::Cleared { pane }) => {
                eprintln!(
                    "drovr: phase '{name}' recorded pane {pane}, which herdr no longer has \
                     — cleared the registration"
                );
            }
            Ok(ReapOutcome::NothingToReap) => {}
            Ok(ReapOutcome::Kept { pane, why }) => eprintln!(
                "drovr: warning: left pane {pane} of finished phase '{name}' open ({}); \
                 `drovr cleanup` will reclaim it",
                match why {
                    PaneKept::CloseFailed(e) => e,
                    PaneKept::Unreadable =>
                        "herdr could not say whether it still exists, and drovr does not \
                         drop a registration it cannot prove is stale"
                            .to_string(),
                }
            ),
            Err(e) => eprintln!(
                "drovr: warning: could not reap finished phase '{name}' ({e}); the phase \
                 that just started is unaffected"
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Session capture
// ---------------------------------------------------------------------------

/// The `CLAUDE_CONFIG_DIR` in effect for a launch happening NOW.
///
/// One function so [`launch_in_pane`] (which inlines it into the agent's
/// environment) and the two sites that RECORD it onto the phase cannot drift:
/// [`crate::run::PhaseAgent::profile`] is only useful if it is exactly the
/// profile the agent authenticated under, and "exactly" is not something two
/// `std::env::var` calls in different files stay agreed on.
fn agent_profile_env() -> Option<String> {
    std::env::var("CLAUDE_CONFIG_DIR").ok()
}

/// What one `pane_info` poll has to say about a phase's persisted record.
///
/// Both fields are "set this if you can", never "set this to whatever you got":
/// a field herdr did not report stays `None` here and is therefore not applied.
/// That is what makes "never clear on absence" a property of the DATA — a
/// `Capture` cannot express "clear it", so no caller can ask for it and no
/// caller has to remember not to.
///
/// Precisely: the guarantee is enforced by [`Capture::apply`], which is the only
/// writer and assigns only inside `if let Some(..)`. It is not something the
/// compiler would flag if a future edit added an `else` arm or a third field
/// that assigns unconditionally. Keep the writing in `apply`, keep it
/// `Some`-gated, and keep the mutation tests that pin it.
struct Capture {
    tab_id: Option<String>,
    session: Option<SessionId>,
}

impl Capture {
    /// Nothing to record. The `Unreadable` answer, and the identity for a phase
    /// that has no pane to poll.
    const NOTHING: Capture = Capture {
        tab_id: None,
        session: None,
    };

    /// Read a poll result, classifying it with [`PaneState`] — the one place in
    /// the tree that distinguishes herdr's three answers. Deriving the same
    /// distinction here from raw `Option`s would be a second classifier of the
    /// same response, and two of those drift.
    ///
    /// `backend` is the backend THIS PHASE's pane runs. It is threaded in
    /// because `AgentSession::resumable_for` is the single chokepoint for the
    /// whole resume rule: kind (`id`, never a transcript path), attribution
    /// (herdr named an owning agent) and match (that agent is this phase's) are
    /// checked by that one call, so none can be skipped by capturing "just the
    /// value".
    fn from_poll(poll: Option<&PaneInfo>, backend: &str) -> Capture {
        match (PaneState::from_poll(poll), poll) {
            // The poll FAILED — herdr unreachable, or the pane gone. It says
            // nothing about the agent, so the phase keeps everything it had.
            // (The second arm is unreachable in practice — `from_poll(None)` is
            // always `Unreadable` — but stating it keeps the match total without
            // an `unreachable!`.)
            (PaneState::Unreadable, _) | (_, None) => Capture::NOTHING,
            // herdr answered; the pane and its tab are there, but no agent
            // session. Record the tab. Do NOT record (or clear) a session:
            // herdr having dropped it is not the agent disowning it.
            (PaneState::NoAgentSession, Some(info)) => Capture {
                tab_id: Some(info.tab_id.as_str().to_owned()),
                session: None,
            },
            // An agent is attached. Record the tab, and the session if it is one
            // a resume could actually use.
            (PaneState::AgentAttached, Some(info)) => Capture {
                tab_id: Some(info.tab_id.as_str().to_owned()),
                session: info
                    .agent_session
                    .as_ref()
                    .and_then(|s| s.resumable_for(backend))
                    .cloned(),
            },
        }
    }

    fn is_empty(&self) -> bool {
        self.tab_id.is_none() && self.session.is_none()
    }


    /// Whether applying this to `p` would change anything.
    ///
    /// Split out from [`Capture::apply`] so a caller can ASK before it commits:
    /// `record_capture` must be able to decide "is there anything to do" without
    /// having already done it, or a persist that fails leaves the in-memory copy
    /// claiming a write that never landed — and every later poll then answers
    /// "nothing to do" and never retries.
    fn would_change(&self, p: &Phase) -> bool {
        let new_tab = matches!(&self.tab_id, Some(t) if p.tab_id.as_deref() != Some(t.as_str()));
        // Fill, never replace. The SAME question `PhaseAgent::record_session`
        // enforces, asked of the same function — so this cannot decide there is
        // work to do that the mutator will then refuse.
        let new_session = self.session.is_some() && p.accepts_captured_session();
        new_tab || new_session
    }

    /// Whether a poll could possibly add anything to `p` — answerable WITHOUT
    /// knowing which backend the phase runs, so it can be asked before the state
    /// read that establishes it.
    ///
    /// **Deliberately optimistic, and the asymmetry is the safety argument.** It
    /// may answer "yes" when the authoritative capture turns out to add nothing
    /// (costing one state read); it must never answer "no" when the capture
    /// would add something. Each arm:
    ///
    /// * the poll reports NO session — nothing to add, ever, since capture never
    ///   clears;
    /// * the poll reports one and the phase holds none — say yes and let the
    ///   real, backend-attributed check decide. This is the arm that keeps a
    ///   stale or missing cached backend harmless: the guard does not consult
    ///   the backend at all here;
    /// * both hold one — nothing to add, because a capture may fill a session
    ///   but never replace a different one
    ///   ([`crate::run::Phase::accepts_captured_session`]). This arm used to compare the
    ///   two through `resumable_for` and say "yes" when they differed, which is
    ///   how an unconfirmed resume came to overwrite the very id it was trying
    ///   to confirm. Not consulting the backend here also makes the guard
    ///   strictly cheaper.
    ///
    /// The tab is compared directly; it is a plain string with no attribution.
    fn might_add(info: &PaneInfo, p: &Phase) -> bool {
        let new_tab = p.tab_id.as_deref() != Some(info.tab_id.as_str());
        // `info.agent_session` is not yet filtered through `resumable_for`, so
        // this asks only whether a session was REPORTED. That is exactly the
        // optimism documented above: a reported session over an absent one may
        // add something; over a present one it never can.
        let new_session = matches!(
            (info.agent_session.as_ref(), p.pane_agent().and_then(|a| a.session())),
            (Some(_), None)
        );
        new_tab || new_session
    }

    /// Apply to `p`, returning whether anything actually changed. Only ever
    /// writes; see the type's note on why there is no clearing path.
    ///
    /// One rule, one writer: the decision is entirely [`Capture::would_change`],
    /// so the two can never disagree about whether there was work to do.
    ///
    /// A session is only ever written into an EXISTING [`crate::run::PhaseAgent`], never by
    /// inventing one — the backend it would have to carry is a fact about the
    /// launch, and `record_capture` is what supplies it.
    fn apply(&self, p: &mut Phase) -> bool {
        if !self.would_change(p) {
            return false;
        }
        if let Some(tab) = &self.tab_id {
            p.tab_id = Some(tab.clone());
        }
        // NOT gated here, deliberately: `PhaseAgent::record_session` refuses to
        // replace a held session itself, so this call is safe to make
        // unconditionally and there is no second copy of the rule to keep in
        // step. (It used to be gated here instead, which is what made the rule
        // caller-specific — the defect round 4 moved onto the mutating API.)
        //
        // Note `would_change` above is true when EITHER the tab or the session
        // is new, so an unconfirmed resume — new pane, hence new tab id, and a
        // stranger's session — does reach this line. It is refused below.
        if let Some(id) = self.session.as_ref() {
            // The return is deliberately ignored, and the reason is an invariant
            // `record_capture` maintains rather than something visible here — so
            // it is written down, because a reader (and a reviewer) otherwise has
            // to re-derive it:
            //
            //   * on the `fresh` copy, the `if pane_agent().is_none() &&
            //     session.is_some() { record_launch(..) }` immediately above the
            //     call guarantees a record exists whenever there is a session;
            //   * on the caller's copy, `adopt_pane_agent` seeds it from `fresh`
            //     — which by then has been through that same block.
            //
            // So `false` is unreachable HERE whenever there is a session to lose.
            // Everywhere else `false` is the right answer: a session without the
            // backend that created it is not a thing this codebase stores.
            //
            // If you add a third `apply` call site, re-establish that guarantee
            // or check the bool.
            p.record_session(id.clone());
        }
        true
    }
}

/// Record what a poll saw onto `phase`, in memory and on disk.
///
/// Reached only through [`poll_phase_pane`], which is what makes it impossible
/// to poll a phase's pane without capturing — see there.
///
/// Called on every poll, i.e. twice a second for the life of a phase, which
/// shapes everything about it:
///
/// * **The write goes through FRESHLY LOADED state, never the caller's copy.**
///   A `phase wait` holds its `RunState` for the whole phase; saving that
///   snapshot mid-poll would restore an hour-old view of the run over whatever
///   else happened meanwhile — precisely the clobber the wait outcomes were
///   reworked to avoid.
/// * **The BACKEND is read from that freshly loaded record too**, not from the
///   caller's copy. The two can disagree: a caller that loaded before the phase
///   was launched has no `PhaseAgent` and would fall back to the run's backend,
///   which is wrong for exactly the case that matters — a reviewer
///   `review_agent_for` put on a different agent. Attributing a session under a
///   guessed backend is how it gets refused and silently dropped.
/// * **Guarded on an actual change**, so the steady state — same tab, same
///   session, poll after poll — writes nothing. A one-hour phase would otherwise
///   rewrite `state.json` 7200 times, and every write is a whole-file write that
///   can lose a concurrent update. The state READ is not guarded: it is far
///   cheaper than the herdr round-trip that just happened, and guarding it was
///   what forced the stale-backend read above.
/// * **Best-effort, and RETRIED.** A failed load or save must leave the caller's
///   copy exactly as it was, so the next poll asks again. That ordering is why
///   [`Capture::would_change`] exists: updating the caller first and persisting
///   second means one transient failure at the moment a session FIRST appears
///   loses it for the rest of the phase. Nothing here may fail the wait — a
///   phase whose agent is working fine does not become a failure because a
///   bookkeeping write did not land.
///
/// Resolves the phase across BOTH lists (reviewers live only in
/// `review_phases`). Safe despite `phase_wait` being deliberately bound to
/// `phases`: `find_phase_mut` searches `phases` first, and
/// `require_name_unclaimed` refuses a name the other list already answers to.
fn record_capture(run: &mut RunState, phase: &str, poll: Option<&PaneInfo>) {
    // Nothing readable → nothing to record, and no reason to touch the disk.
    let Some(info) = poll else {
        return;
    };
    // The cheap guard: ASK the caller's own copy whether this poll could add
    // anything, before reading state. The steady state — same tab, same session,
    // poll after poll — does no I/O at all. See `Capture::might_add` for why a
    // stale cached backend can only cost an extra read here, never a lost
    // capture; that asymmetry is what lets the guard run before the load that
    // establishes the authoritative backend.
    match run.find_phase(phase) {
        Some(p) if Capture::might_add(info, p) => {}
        _ => return,
    }
    let mut fresh = match RunState::load(&run.name) {
        Ok(fresh) => fresh,
        Err(e) => {
            // Carry the reason. Capture failures are invisible by design
            // (best-effort, never fails a wait), so the message is the only
            // thing a human will ever have when a rehydrate later finds nothing
            // to resume — "could not be re-read" alone names no cause.
            capture_write_failed(
                &run.name,
                phase,
                &format!("its run state could not be re-read ({e})"),
            );
            return;
        }
    };
    // A phase absent from freshly loaded state has nowhere to record this, and
    // capture must not resurrect it.
    let Some(target) = fresh.find_phase(phase) else {
        return;
    };
    // THIS PHASE's backend, from the record just loaded. A reviewer's is chosen
    // by `Config::review_agent_for` and legitimately differs from the run's.
    //
    // A phase with no `PhaseAgent` was launched by a build that did not record
    // one; `RunState::agent` is then the best evidence available, and it is
    // right for every pipeline phase. It deserializes to `Some("claude")` for
    // runs older than that field, so there is no third tier. When even that is
    // absent nothing is recorded: a guessed backend is how a session gets
    // captured under the wrong agent.
    // BOTH branches read `fresh`, never the caller's copy. Round 2 fixed the
    // phase-level source and left this legacy fallback resolving from the
    // caller's `run.agent` — the same stale-source bug, one branch over. When
    // the rule is "attribute from what is on disk", it has to hold for every
    // branch that resolves the value, not the one that was pointed at.
    let Some(backend) = target
        .pane_agent()
        .map(|a| a.backend().to_owned())
        .or_else(|| fresh.agent.clone())
    else {
        // The last silent skip, now audible. Refusing to guess is right — a
        // guessed backend is how a session gets attributed to the wrong agent —
        // but refusing SILENTLY means the operator learns about it from a
        // rehydrate that finds nothing, long after the pane is gone.
        capture_write_failed(
            &run.name,
            phase,
            "neither the phase nor the run records which agent backend it runs, and a \
             guessed one would attribute the session to the wrong agent",
        );
        return;
    };
    let capture = Capture::from_poll(Some(info), &backend);
    if capture.is_empty() {
        return;
    }
    let Some(p) = fresh.find_phase_mut(phase) else {
        return;
    };
    // A phase with no `PhaseAgent` on disk gets one now, carrying the backend
    // resolved above — otherwise `apply` has nowhere to put the session and the
    // capture would be silently dropped for exactly the legacy phases the
    // fallback exists to serve.
    //
    // `profile: None` here is CORRECT and deliberate — reviewed and declined as
    // a finding, recorded so it is not re-raised. A phase launched before
    // profile capture existed genuinely has no recorded profile, and `None`
    // already means "the default profile", which is what such a launch used.
    // The tempting alternative — filling it from the CURRENT environment — is a
    // guess: whatever `CLAUDE_CONFIG_DIR` this process happens to hold need not
    // be the one that pane authenticated under, and a wrong profile resolves to
    // the wrong `projects/<escaped-cwd>/` directory and silently finds no
    // session at all. An honest "unknown" degrades to a reseed; a confident
    // wrong answer degrades to a mystery.
    if p.pane_agent().is_none() && capture.session.is_some() {
        p.record_launch(backend, None);
    }
    // `apply` is false when the freshly-loaded state ALREADY carries what this
    // poll saw — another writer got there first, or the caller's copy had simply
    // fallen behind. Nothing to write; the adoption below still runs, which is
    // what stops the caller re-asking this question on every subsequent poll.
    if capture.apply(p)
        && let Err(e) = fresh.save()
    {
        capture_write_failed(
            &run.name,
            phase,
            &format!("its run state could not be saved ({e})"),
        );
        return;
    }
    // Only now, with the value on disk, does the caller's copy adopt it — so a
    // failure above leaves that copy stale on purpose, and the next poll retries.
    //
    // The caller's `PhaseAgent` is seeded from what is now ON DISK, never rebuilt
    // here. Another writer may have recorded a profile (or a backend) this
    // process never saw, and re-deriving one would hand the caller a
    // `profile: None` that its next `run.save()` would write over the real one.
    //
    // "Only when the caller has none" is `seed_pane_agent`'s own rule now, not
    // a test written here: it used to be a `(None, Some(_))` match at this call
    // site, which left the API able to replace a record — session and all —
    // for anyone who did not think to write the same match.
    let persisted_agent = fresh
        .find_phase(phase)
        .and_then(|p| p.pane_agent().cloned());
    if let Some(p) = run.find_phase_mut(phase) {
        if let Some(agent) = persisted_agent {
            p.seed_pane_agent(agent);
        }
        capture.apply(p);
    }
}

/// Say ONCE, per run+phase, that a capture could not be persisted.
///
/// Capture is best-effort and must never fail a wait, but "best-effort" was
/// silent, and the failure it hides is not recoverable on its own: if the agent
/// exits before a later poll retries, the session is gone for good and the only
/// symptom is a rehydrate, much later, that has nothing to resume. One line at
/// the moment it happens is the difference between a diagnosable problem and a
/// mystery.
///
/// Once per run+phase, not per poll: this sits in a loop that runs twice a
/// second, and a repeating diagnostic is one nobody reads. Bounded the same way
/// `herdr.rs` bounds its per-pane sets, and for the same reason — the always-on
/// review server never restarts.
fn capture_write_failed(run_name: &str, phase: &str, why: &str) {
    static WARNED: std::sync::Mutex<std::collections::BTreeSet<String>> =
        std::sync::Mutex::new(std::collections::BTreeSet::new());

    if !first_capture_failure_for(&WARNED, &format!("{run_name}/{phase}")) {
        return;
    }
    eprintln!(
        "drovr: phase '{phase}' of run '{run_name}': could not record the agent's session \
         because {why}. Continuing — this never fails a phase — but the session id is only \
         readable while the agent is ALIVE, so if it exits before a later poll succeeds, \
         rehydrating this phase will have nothing to resume and will fall back to a fresh \
         agent."
    );
}

/// Whether this is the first capture failure seen for `key` (a `run/phase`).
///
/// Takes the set as a parameter — like `herdr::first_time_for`, and for the same
/// reason: a gate wired to a `static` cannot be tested, and this one guards a
/// diagnostic that only ever fires in the degraded case nobody exercises by
/// hand. BOUNDED, and it clears wholesale rather than evicting: it is a
/// de-duplication gate, not a cache, so no entry is worth more than another.
fn first_capture_failure_for(
    seen: &std::sync::Mutex<std::collections::BTreeSet<String>>,
    key: &str,
) -> bool {
    /// Same reasoning as `herdr::WARNED_PANES_CAP`: the always-on review server
    /// never restarts, so an unbounded set is a slow leak in the wrong place.
    const CAP: usize = 512;
    let mut seen = seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if seen.len() >= CAP && !seen.contains(key) {
        seen.clear();
    }
    seen.insert(key.to_string())
}

/// **Poll a phase's pane. THE only way to do it.**
///
/// Returns exactly what [`Herdr::pane_info`] returned, and records what it saw
/// onto the phase on the way past. Capture is welded to the poll here on
/// purpose: it used to live inside the two wait loops, and the third poll site
/// — `code_review`'s reviewer wait — simply did not have it. That gap was
/// invisible and permanent. herdr reports `agent_session` only while the agent
/// is alive; a reviewer is explicitly told to exit, its readiness gate returns
/// on the FIRST poll that reports "started" (which may carry no session yet),
/// and reaping closes its pane. So a reviewer whose session showed up one poll
/// later than its status simply never had one recorded, and nothing downstream
/// could tell.
///
/// **Do not call `Herdr::pane_info` directly for a pane that belongs to a
/// phase.** A second capture site is a second thing to forget; this is one.
///
/// Returns `None` when the phase has no pane at all, which is indistinguishable
/// here from a failed poll — both mean "no information", and neither is
/// something to record.
pub(crate) fn poll_phase_pane<H: Herdr>(
    h: &H,
    run: &mut RunState,
    phase: &str,
) -> Option<PaneInfo> {
    // The pane is DERIVED from the phase, never passed alongside it. As two
    // parameters a caller could hand over a phase and some other phase's pane,
    // and the capture would attribute that pane's session to this phase —
    // permanently, because capture is one-shot: herdr drops the session when the
    // agent exits, so there is no later poll to correct it. It is also silent,
    // and reaping runs off the record it would corrupt. One argument, no pair to get
    // wrong.
    let pane_id = run.find_phase(phase).and_then(|p| p.pane_id())?.to_owned();
    let info = h.pane_info(&pane_id);
    record_capture(run, phase, info.as_ref());
    info
}

/// What a handoff scan concluded. Two rules, no markdown model — and one variant per
/// outcome, so "passes" is a state the type names rather than one a reader infers from an
/// empty vec.
#[derive(Debug, PartialEq, Eq)]
enum HandoffShape {
    /// Nothing beyond what drovr itself wrote: the agent never touched the scaffold.
    ///
    /// Carries the sections still holding a placeholder, when there are any. They do not
    /// change the refusal — "the whole file is scaffold" is the useful thing to say — but
    /// discarding a fact already computed leaves the variant unable to answer a question a
    /// caller may reasonably ask.
    Untouched { placeholders: Vec<String> },
    /// The agent wrote something, but left the placeholder in these sections.
    Placeholders(Vec<String>),
    /// Written, with no placeholder left. Passes.
    Complete,
}

/// Decide whether a handoff was actually written, WITHOUT parsing markdown.
///
/// Five rounds of review on a per-section body model produced seven bypasses and four false
/// refusals — fence state, comment state, chained comments, indented headings, `#`-led
/// lines, then verbatim guidance matching and heading-presence. Each fix's seams became the
/// next round's findings. The lesson is that "did this section receive substance" is not
/// decidable from markdown by any rule simple enough to be correct.
///
/// So the gate stops trying, and asks two things it CAN answer exactly:
///
/// 1. **Is the file nothing but what drovr wrote?** Every non-blank line appears in
///    `handoff_scaffold()`'s output. That is the accident this gate exists for — scaffold,
///    forget, signal done.
/// 2. **Does any line still read exactly `TODO`, at column 0?** That is the placeholder
///    drovr wrote, still sitting where a section's content belongs. An indented one is
///    quoted text, which also makes indenting the escape from a false positive.
///
/// Both are cheap, neither can misread structure, and every refusal is escapable by editing
/// the line the message names.
///
/// **What this deliberately does NOT catch**, because five rounds showed the cost of trying:
/// an agent that fills one section and deletes the other six blocks; a body of
/// `TODO: fill this in`, or a lookalike using non-ASCII characters. An agent set on evading
/// the gate can; the gate is here for the one that forgot. `drovr collect` shows the next
/// phase exactly what it inherited, which is the real check on a thin handoff.
fn scan_handoff(contents: &str) -> HandoffShape {
    // ONE generation of the scaffold, borrowed twice — the two helpers each built it
    // separately, which was wasteful and, worse, two chances for them to disagree about
    // what the scaffold contains.
    let scaffold = crate::brief::handoff_scaffold();
    let scaffold_lines: Vec<&str> = scaffold
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let headings: Vec<&str> = scaffold
        .lines()
        .filter_map(|l| l.strip_prefix("## ").map(str::trim))
        .collect();

    let mut wrote_something = false;
    let mut placeholders = Vec::new();
    let mut section: Option<&str> = None;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("## ")
            && let Some(h) = headings.iter().find(|h| **h == rest.trim())
        {
            section = Some(h);
        }
        // Compared UNTRIMMED: the scaffold writes the placeholder at column 0, so an
        // indented `TODO` is quoted text — and indenting is then a real escape from the
        // refusal, which a trimmed comparison silently denied.
        if line == crate::brief::SCAFFOLD_PLACEHOLDER {
            let name = section.unwrap_or("(before the first section)").to_string();
            if !placeholders.contains(&name) {
                placeholders.push(name);
            }
            continue;
        }
        if !scaffold_lines.contains(&trimmed) {
            wrote_something = true;
        }
    }

    // Nothing outside drovr's own text — whether the placeholders are still there or the
    // agent deleted them and wrote nothing, it is the same accident.
    if !wrote_something {
        return HandoffShape::Untouched { placeholders };
    }
    if placeholders.is_empty() {
        return HandoffShape::Complete;
    }
    HandoffShape::Placeholders(placeholders)
}

/// Whether `status` (a pane's herdr `agent_status`) means the agent has STARTED
/// AND is at its composer, so a prompt sent now will land. `idle`, `working`, and
/// `done` qualify; `None` (unreadable), `"unknown"`, and `"blocked"` do not.
///
/// Deliberately NOT limited to `idle`: a `working` agent has a composer too (the
/// prompt just queues), so gating on `idle` alone would make `phase_send` block
/// the full timeout on any follow-up send to a busy agent, which never returns to
/// `idle` mid-task. We only need "has it started", not "is it free".
///
/// `blocked` is deliberately EXCLUDED even though it means the agent attached: a
/// pane blocked on a first-run/trust/permission prompt is NOT sitting at its
/// composer, so `agent.prompt` would type into that dialog — corrupting it and
/// swallowing the seed (the original flake, wearing a new hat). Treating `blocked`
/// as not-ready lets the gate wait it out; if it never clears, `phase_send` raises
/// `TimedOut`, which the CLI surfaces via `diagnose_stuck_phase` so a human can
/// answer the prompt.
///
/// An `AgentStatus::Other` — a herdr state this drovr has never seen — is NOT
/// treated as started: an unrecognised state is not evidence the composer is
/// ready, and waiting it out is recoverable where typing into it is not.
fn agent_has_started(status: Option<&AgentStatus>) -> bool {
    matches!(
        status,
        Some(AgentStatus::Idle) | Some(AgentStatus::Working) | Some(AgentStatus::Done)
    )
}

/// Poll until the agent in `pane_id` has STARTED, returning `true` once it has,
/// or `false` if `timeout` elapses first. A freshly-spawned `claude` needs a
/// moment to boot its TUI and attach its integration; `agent.prompt` types AND
/// submits natively, so firing it into a still-booting pane drops or garbles the
/// prompt — the "phase send is flaky" race. herdr reports a definite
/// `agent_status` once the agent has attached (see `agent_has_started`), so poll
/// for that. The query is the same read-only `pane get` `phase_wait` polls, so it
/// never moves focus.
///
/// A `false` return is NOT ignored by the caller: `phase_send` treats "never
/// became ready" as a failure to RAISE (the agent is likely parked on a first-run
/// or permission prompt with no human at the pane), rather than blindly sending a
/// prompt the agent can't receive.
fn wait_agent_ready<H: Herdr>(
    h: &H,
    run: &mut RunState,
    phase: &str,
    timeout: Duration,
    poll_interval: Duration,
) -> bool {
    wait_agent_ready_until(
        h,
        run,
        phase,
        Polling::until(Instant::now() + timeout, poll_interval),
    )
    .is_some()
}

/// [`wait_agent_ready`] against an absolute deadline, returning the `PaneInfo`
/// of the poll that saw the agent start.
///
/// The info is returned rather than discarded because "the agent is up" and
/// "the agent is in the conversation you asked for" are different questions
/// answered by the *same* `pane.get`, and rehydrate has to ask both. A deadline
/// rather than a duration so a caller can spend one budget across both without
/// the second wait silently doubling the first.
fn wait_agent_ready_until<H: Herdr>(
    h: &H,
    run: &mut RunState,
    phase: &str,
    polling: Polling,
) -> Option<PaneInfo> {
    loop {
        // Exactly ONE poll per iteration, and it captures on the way past.
        // Narrow to the status inline rather than through a helper, so "the poll
        // failed" cannot be confused with a status.
        let info = poll_phase_pane(h, run, phase);
        if agent_has_started(info.as_ref().and_then(|i| i.agent_status.as_ref())) {
            return info;
        }
        if !polling.sleep() {
            return None;
        }
    }
}

/// A polling budget: when to give up, and how long to wait between attempts.
///
/// The two always travel together — a deadline with someone else's interval is
/// a wait that either spins or overshoots — so they cross a boundary as one
/// value rather than as two parameters a caller pairs up. It also keeps the
/// rehydrate waits on ONE budget: confirmation is not a second timeout bolted
/// onto readiness.
#[derive(Debug, Clone, Copy)]
struct Polling {
    deadline: Instant,
    interval: Duration,
}

impl Polling {
    fn until(deadline: Instant, interval: Duration) -> Polling {
        Polling { deadline, interval }
    }

    /// The same budget, but never leaving less than `floor` from now.
    ///
    /// EXTENDS only — a budget with time left is returned unchanged, so this
    /// cannot be used to cut a wait short.
    fn with_floor(self, floor: Duration) -> Polling {
        Polling {
            deadline: self.deadline.max(Instant::now() + floor),
            ..self
        }
    }

    /// Sleep until the next attempt, or return `false` when the budget is spent.
    /// Never sleeps past the deadline.
    fn sleep(&self) -> bool {
        let now = Instant::now();
        if now >= self.deadline {
            return false;
        }
        thread::sleep(self.interval.min(self.deadline - now));
        true
    }
}

/// ⭐ **What drovr LEARNED about the agent it launched onto the new pane — the
/// three epistemic states, kept apart because they authorise different things.**
///
/// This is an enum rather than a `Result<(), Option<SessionId>>` because the
/// decision that reads it is *whether to destroy a live pane*, and the rule is:
///
/// > **drovr surrenders a pane only when it has SEEN a session that is not the
/// > one it expected. Not when it has seen nothing.**
///
/// Stated in terms of the outcome variants instead of in terms of the evidence,
/// that rule was applied one gate too narrowly once already: `NeverReady` was
/// correctly spared, and then every unconfirmed resume — including the one that
/// observed nothing at all — was surrendered anyway. An `Option` invites
/// `is_none()` to be forgotten; an exhaustive `match` on named states does not,
/// and a fourth state could not be added without every reader handling it.
#[derive(Debug, PartialEq, Eq)]
enum ResumeEvidence {
    /// herdr reported the session drovr asked it to resume. The conversation is
    /// back.
    Confirmed,
    /// herdr reported a DIFFERENT session. **Positive evidence** that the pane
    /// is running a conversation this phase's record does not describe — the
    /// only thing that authorises closing it.
    Contradicted(SessionId),
    /// herdr never reported a session at all within the budget. The agent may
    /// be perfectly resumed and merely slow to surface its id (herdr reports
    /// `Idle` before `agent_session` appears — see
    /// `a_session_that_shows_up_a_poll_late_is_still_a_confirmed_resume`), or
    /// the pane may be in trouble. **drovr does not know**, and this branch is
    /// built on "absence of confirmation is not evidence", so nothing is
    /// destroyed. Epistemically identical to `NeverReady`, and it takes the
    /// same branch.
    Unobserved,
}

/// Poll until herdr reports the agent on this phase's pane carrying `expected`
/// — i.e. until the resume is *confirmed* — or the deadline passes.
///
/// Returns `Ok(())` on confirmation, `Err(last_observed)` otherwise.
///
/// **Why this exists at all.** `wait_agent_ready` proves an agent is up. It
/// proves nothing about WHICH conversation it is in, and a recorded session id
/// that no longer resolves (pruned session file, cleared profile storage, a
/// changed id format) makes the backend start a FRESH one — attached, idle,
/// indistinguishable from a successful resume from the outside. The ⟳'s whole
/// promise is that the actual conversation returns, so `Resumed` is claimed on
/// herdr reporting the id back and on nothing weaker.
///
/// It keeps looking rather than judging one sample: the readiness gate returns
/// on the FIRST poll that reports "started", and herdr does not necessarily
/// carry an `agent_session` that early — the same one-poll lag that used to
/// cost reviewers their captured sessions.
fn confirm_resumed_session<H: Herdr>(
    h: &H,
    run: &mut RunState,
    phase: &str,
    backend: &str,
    expected: &SessionId,
    first: Option<PaneInfo>,
    polling: Polling,
) -> ResumeEvidence {
    // `resumable_for` is the same chokepoint a capture goes through: a path
    // session, one herdr attributes to a different agent, or none at all are
    // all "not the id we asked for".
    let observed_in = |info: &Option<PaneInfo>| -> Option<SessionId> {
        info.as_ref()
            .and_then(|i| i.agent_session.as_ref())
            .and_then(|s| s.resumable_for(backend))
            .cloned()
    };
    let mut last = observed_in(&first);
    loop {
        if last.as_ref() == Some(expected) {
            return ResumeEvidence::Confirmed;
        }
        if !polling.sleep() {
            return match last {
                Some(other) => ResumeEvidence::Contradicted(other),
                None => ResumeEvidence::Unobserved,
            };
        }
        let info = poll_phase_pane(h, run, phase);
        // Never overwrite something seen with nothing: an unreadable poll says
        // no more about the session than it does about the agent.
        if let Some(seen) = observed_in(&info) {
            last = Some(seen);
        }
    }
}

/// Number of trailing non-empty pane lines treated as "the composer region".
/// Every agent TUI puts its input box at the bottom, just above the status bar.
/// Bounding the search there stops a payload echoed into SCROLLBACK by an earlier
/// send from reading as evidence that THIS one landed.
const COMPOSER_TAIL_LINES: usize = 8;

/// What `claude` and `cursor` both render in place of a large pasted payload
/// (`[Pasted text #1 +124 lines]`) instead of echoing it. Matched
/// case-insensitively.
const PASTE_PLACEHOLDER: &str = "pasted text";

/// Shortest payload prefix accepted as verbatim evidence. Below this, a fragment
/// is too generic to tell the payload apart from ordinary pane chrome.
const MIN_VERBATIM_EVIDENCE: usize = 12;

/// Longest payload prefix compared. Capped so a composer that truncates or wraps
/// a long first line still matches it.
const MAX_VERBATIM_EVIDENCE: usize = 40;

/// Does `pane` show POSITIVE evidence that `text` reached the agent's composer?
///
/// The caller uses this to decide whether pressing Enter is safe, so the test is
/// deliberately one-sided: "not sure" must read as "no". A missed nudge is an
/// error a human resolves; a wrong nudge answers whatever dialog is on screen.
///
/// Two shapes count as evidence, because they are what the two agents actually
/// render for a pending prompt:
///   * a bracketed-paste placeholder — how both collapse a large payload, which
///     is the normal case for a phase briefing; and
///   * a verbatim prefix of the payload's first line, for a prompt short enough
///     to be echoed as typed.
///
/// A before/after pane DIFF is deliberately NOT used, and that is the whole
/// reason this function exists. Agent UIs mutate their own chrome between reads —
/// status bars, token counters, spinner frames, and a welcome screen that finishes
/// painting seconds after launch. "Something changed" is therefore true even when
/// the payload was swallowed whole by a modal, which made an earlier version of
/// this check press Enter on claude's "New MCP server" approval and accept it.
/// What one look at the composer region established about `text`.
///
/// Three states, not a `bool`, because the third one is a different FACT and it
/// sends a human somewhere else: "I looked and the payload is not there" points
/// at a dialog on the screen, while "I could not look" points at herdr. Both
/// forbid the nudge — but a `bool` can only carry one of them, and whichever it
/// borrows makes `phase_send` assert a cause it has no evidence for. That is the
/// same confidently-wrong diagnosis this whole change exists to remove.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ComposerEvidence {
    /// The payload's signature is in the composer region right now.
    Present,
    /// The pane was read, and the payload is not in the composer region.
    Absent,
    /// The pane could not be read at all, so nothing was established.
    Unreadable,
}

/// Read `pane_id` once and classify what the composer region says about `text`.
fn read_composer_evidence<H: Herdr>(h: &H, pane_id: &str, text: &str) -> ComposerEvidence {
    match h.agent_read(pane_id) {
        Ok(pane) if pane_shows_payload(&pane, text) => ComposerEvidence::Present,
        Ok(_) => ComposerEvidence::Absent,
        Err(_) => ComposerEvidence::Unreadable,
    }
}

fn pane_shows_payload(pane: &str, text: &str) -> bool {
    let tail = tail_snippet(pane, COMPOSER_TAIL_LINES);
    if tail.to_lowercase().contains(PASTE_PLACEHOLDER) {
        return true;
    }
    let Some(first_line) = text.lines().map(str::trim).find(|l| !l.is_empty()) else {
        return false;
    };
    let fragment: String = first_line.chars().take(MAX_VERBATIM_EVIDENCE).collect();
    fragment.chars().count() >= MIN_VERBATIM_EVIDENCE && tail.contains(&fragment)
}

/// Deliver `text` to the running phase pane and CONFIRM the agent actually
/// started on it. Returns `Ok` only when herdr observed the agent move; every
/// other path raises rather than reporting a success the seed never had.
///
/// Four things can go wrong, and they need different answers:
///
/// 1. **The agent never attaches.** Gated up front by `wait_agent_ready`; raises
///    [`io::ErrorKind::TimedOut`] without sending, and without re-opening.
/// 2. **The prompt cannot be delivered at all** — herdr is unreachable, the pane
///    is gone. The transport error propagates.
/// 3. **The prompt is swallowed.** `agent_status` is not trustworthy evidence
///    that a pane is at its composer: a pane parked on a dialog herdr's detection
///    manifest does not classify reports `idle`, not `blocked` (claude's "New MCP
///    server" approval is the proven case). The readiness gate waves that
///    through, `agent.prompt` reports success, and the payload vanishes.
/// 4. **The prompt lands but is never submitted.** The payload sits in the
///    composer indefinitely and the agent never starts. This is the common case
///    on `cursor` and it happens on `claude` too; it is a race, not a function of
///    the payload (see `docs/known-issues.md`).
///
/// Cases 3 and 4 both look like "herdr saw no state change", so
/// [`Herdr::agent_prompt_confirm`] detects them together — and
/// [`pane_shows_payload`] is what tells them apart, by looking for the payload in
/// the composer. It is evaluated BEFORE and after the prompt, and only evidence
/// that *appeared* counts: a long-lived pane can be sent to repeatedly, and an
/// earlier briefing still sitting in the composer region must not be mistaken for
/// this one arriving.
///
/// That distinction is load-bearing, not cosmetic: case 4 is fixed by pressing
/// Enter, and case 3 must NEVER be, because the keystroke would land on whatever
/// dialog is up and accept its highlighted option. So the nudge requires positive
/// evidence the payload is in the composer; no evidence (including a pane that
/// cannot be read) means raise, never guess.
///
/// Keep the guard and the recovery distinct. The observed state transition is the
/// one authoritative verdict on whether the seed arrived. The composer-evidence
/// check is NOT a second opinion on that — it only decides whether nudging is
/// safe. Never use evidence to conclude "it probably worked".
///
/// Takes `&mut RunState` because sending to a FINISHED phase re-opens it. This is
/// the pipeline's documented re-entry path — `skills/pipeline/SKILL.md`: "Re-entry
/// needs **no `drovr phase start`** … `drovr phase send` reaches it directly" —
/// and it is how the implement↔review loop drives an exit-3 iteration. Without the
/// re-open, the previous iteration's `Done` status and completion marker both
/// survive, so the `phase wait` that follows the send returns `Done` in
/// microseconds while the agent has not yet read the prompt, and the driver
/// advances — and then the next `phase start` reaps a pane the driver had just
/// messaged.
pub fn phase_send<H: Herdr>(h: &H, run: &mut RunState, phase: &str, text: &str) -> io::Result<()> {
    phase_send_with_timeout(
        h,
        run,
        phase,
        text,
        SEND_READY_TIMEOUT,
        POLL_INTERVAL,
        SEND_CONFIRM_TIMEOUT,
    )
}

/// Mark a PIPELINE phase live again for work being requested NOW: drop any
/// completion marker and set the status back to `Running`, so the `phase_wait`
/// that follows the send waits for this request instead of reporting an earlier
/// one's completion.
///
/// The agent is the SAME process across a send re-entry (same pane, same
/// `DROVR_PASS`), so the pass token cannot distinguish the two passes here — only
/// clearing the previous completion can.
///
/// Both the sweep and the status reset are UNCONDITIONAL, not gated on
/// `status == Done`. A marker sits on disk with a matching token during the whole
/// interval between "the agent wrote it" and "some `phase_wait` consumed it", and
/// if no wait was running that interval is unbounded — the status is still
/// `Running`. Gating on `Done` would skip the sweep in exactly that state and the
/// next wait would complete instantly off a marker that predates the send. Any
/// marker present at send time necessarily records work finished BEFORE the
/// request being made now, so discarding it is always right.
///
/// Reviewer phases no-op here: they live in `review_phases`, `find_phase_idx`
/// searches `phases` only, and `phase_wait` never runs on them.
///
/// Returns whether it ACTED. `false` is the reviewer no-op above, and the caller
/// needs it: `phase_send` reports what a failed delivery left behind, and a
/// reviewer phase was left exactly as it was — marker intact, status untouched.
/// Claiming otherwise would tell a human their completed reviewer had been
/// reset.
fn reopen_for_re_entry(run: &mut RunState, phase: &str) -> io::Result<bool> {
    let Some(i) = find_phase_idx(run, phase) else {
        return Ok(false);
    };
    // ORDER MATTERS, and it is the OPPOSITE of `phase_start`'s — because here the
    // token does NOT change. The same agent serves both passes, so the marker is
    // the only thing that distinguishes them, and the rule is unchanged: no
    // failure may leave a state where `phase_wait` completes without evidence of
    // the work being requested now.
    //   * sweep fails → nothing was touched: old status + old marker, mutually
    //     consistent, describing a pass that did finish. The send returns Err.
    //   * save fails  → the marker is already gone, so `phase_wait` finds no
    //     evidence and times out honestly, whatever the stale status says.
    // Persist-first would be wrong here: it would leave `Running` (new work
    // intended) next to a marker whose token still MATCHES, and `phase_wait`
    // would complete off work that predates the send.
    remove_stale_marker(&run.name, phase)?;
    if run.phases[i].status != PhaseStatus::Running {
        run.phases[i].status = PhaseStatus::Running;
    }
    // Preserving: the re-open above is a blocking herdr round trip, so this
    // snapshot can be older than an archive the human performed during it.
    run.save_preserving_archived()?;
    Ok(true)
}

/// What a failed `phase_send` LEFT BEHIND, given whether `reopen_for_re_entry`
/// acted. Every failure after the re-open has to say this, not just the transport
/// one: the previous pass's completion marker is already deleted and the status is
/// back to `Running`, so a caller told only "the seed did not arrive" reads that
/// phase as work in progress forever — the phantom-incomplete-phase state. Name
/// the way out too (re-send; the re-open is idempotent).
///
/// When the re-open did NOT act — a reviewer phase, which lives in
/// `review_phases` — nothing was touched and the message must not say otherwise.
/// A reviewer that has finished and exited is precisely the pane a send fails
/// against, and its marker is intact: telling its human it had been reset would be
/// a false report about a phase that is correctly complete.
fn send_failure_aftermath(reopened: bool) -> &'static str {
    if reopened {
        "but this phase had ALREADY been re-opened for it — its completion marker is deleted \
         and its status is back to Running, so it now looks like work in progress that nobody \
         was asked to do. Re-send once the pane is reachable (re-opening again is harmless), \
         or mark the phase failed."
    } else {
        "nothing was changed — this phase is not one `phase send` re-opens, so its status and \
         any completion it already recorded are untouched. Re-send once the pane is reachable."
    }
}

/// Assemble a post-re-open send failure: what went wrong, then what it left
/// behind. Shared by every failure path after `reopen_for_re_entry` so none of
/// them can quietly omit the aftermath.
fn send_failure(
    run: &RunState,
    phase: &str,
    reopened: bool,
    kind: io::ErrorKind,
    what: &str,
) -> io::Error {
    io::Error::new(
        kind,
        format!(
            "phase '{phase}' of run '{run_name}': {what}, {aftermath}",
            run_name = run.name,
            aftermath = send_failure_aftermath(reopened),
        ),
    )
}

/// [`phase_send`] with injectable timeouts + poll interval (so tests can exercise
/// the not-ready and undelivered paths, and the poll loop, without waiting out the
/// full production timeouts or the real 500ms poll cadence).
fn phase_send_with_timeout<H: Herdr>(
    h: &H,
    run: &mut RunState,
    phase: &str,
    text: &str,
    ready_timeout: Duration,
    poll_interval: Duration,
    confirm_timeout: Duration,
) -> io::Result<()> {
    require_phase_name(phase)?;
    let pane_id = require_pane_id(run, phase)?;
    if !wait_agent_ready(h, run, phase, ready_timeout, poll_interval) {
        // Render sub-second timeouts as ms so an injected test timeout doesn't
        // print a misleading "within 0s"; production (30s) reads "within 30s".
        let waited = if ready_timeout.as_secs() >= 1 {
            format!("{}s", ready_timeout.as_secs())
        } else {
            format!("{}ms", ready_timeout.as_millis())
        };
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "agent for phase '{phase}' of run '{run_name}' did not become ready \
                 within {waited} — not sending. It is likely parked on a first-run or \
                 permission prompt with no human at the pane. Attach to check: {attach}",
                run_name = run.name,
                attach = attach_command(&run.name),
            ),
        ));
    }
    // Re-open ONLY now. The agent is at its composer and has not seen `text`, so
    // any marker on disk is from earlier work and is safe to discard — while
    // sweeping before the readiness gate would destroy the record of a genuine
    // completion on every failed send (a phase parked on a permission prompt
    // would lose its `Done` and its marker without any new work being requested).
    let reopened = reopen_for_re_entry(run, phase)?;

    // Snapshot FIRST, so evidence found afterwards can be attributed to THIS send.
    // A pane that already shows the payload's signature — a long-lived pane whose
    // previous briefing is still in the composer region, or one the browser
    // mirror's fire-and-forget `/send` typed into — cannot produce fresh evidence,
    // and must not be nudged on the strength of someone else's paste.
    let evidence_before = read_composer_evidence(h, &pane_id, text);

    let outcome = h
        .agent_prompt_confirm(&pane_id, text, confirm_timeout)
        .map_err(|e| {
            send_failure(
                run,
                phase,
                reopened,
                e.kind(),
                &format!("the prompt could not be delivered ({e})"),
            )
        })?;
    if outcome == PromptOutcome::Started {
        return Ok(());
    }

    // The agent did not move. Only nudge if the payload is demonstrably sitting in
    // the composer NOW and was not there before — see `pane_shows_payload` for why
    // this must be positive evidence rather than "the pane changed".
    let evidence_after = read_composer_evidence(h, &pane_id, text);
    // Exactly ONE pairing licenses the keystroke: the composer was looked at
    // before and did NOT hold the payload, and holds it now. Spelled as a match on
    // both values rather than `after == Present && before != Present`, because
    // that shorthand quietly admits `Unreadable` before — a look that never
    // happened cannot establish that the marker is new, and the payload may have
    // been sitting there the whole time.
    let landed_in_composer = matches!(
        (evidence_before, evidence_after),
        (ComposerEvidence::Absent, ComposerEvidence::Present)
    );

    if !landed_in_composer {
        // Same refusal, four different reasons — and the reason is the whole
        // value of the message, because it is what tells the human where to look.
        // Do not collapse these: asserting "it was swallowed" for a pane drovr
        // could not read, or for one visibly holding a paste marker, is a
        // confident diagnosis with nothing behind it.
        //
        // Deliberately NO `_` arm. A catch-all is what let `(Unreadable, Present)`
        // inherit the swallow narrative in the first place; matching every pair
        // means a new [`ComposerEvidence`] variant fails to compile here instead
        // of silently acquiring whichever story happens to be last.
        let why = match (evidence_before, evidence_after) {
            (_, ComposerEvidence::Unreadable) => format!(
                "the seed was NOT delivered — herdr saw no state change after the prompt, and \
                 the pane could not be READ, so drovr cannot tell whether the payload is \
                 sitting unsubmitted in the composer or was swallowed by a dialog. \
                 Deliberately NOT pressing a key blind: on a dialog, Enter accepts its \
                 highlighted option on your behalf. Check that herdr can see the pane, then \
                 look at it: {attach}",
                attach = attach_command(&run.name),
            ),
            (ComposerEvidence::Present, ComposerEvidence::Present) => format!(
                "the seed was NOT delivered — herdr saw no state change after the prompt. The \
                 composer does hold a payload signature, but it was ALREADY there before this \
                 prompt, so it is someone else's paste (an earlier send, or the browser \
                 mirror) and proves nothing about this one. Deliberately NOT pressing a key: \
                 stale text in the composer cannot rule out a dialog now on top of it. Clear \
                 the composer, then re-send: {attach}",
                attach = attach_command(&run.name),
            ),
            (ComposerEvidence::Unreadable, ComposerEvidence::Present) => format!(
                "the seed was NOT delivered — herdr saw no state change after the prompt. The \
                 composer DOES hold a payload signature, but the pane could not be read before \
                 the prompt, so drovr cannot tell whether it is this one or something that was \
                 already sitting there. Deliberately NOT pressing a key on an undated payload: \
                 if it is not yours, Enter goes to whatever is actually on screen. Look at the \
                 pane — if that is your seed, submit it by hand: {attach}",
                attach = attach_command(&run.name),
            ),
            // Everything left has `after == Absent`: drovr looked, and the
            // composer does not hold the payload. `(Absent, Present)` is the nudge
            // path, returned above; it is named only to keep this match total.
            (_, ComposerEvidence::Absent)
            | (ComposerEvidence::Absent, ComposerEvidence::Present) => {
                format!(
                    "the seed was NOT delivered — herdr saw no state change after the prompt, \
                     and the payload is nowhere in the agent's composer, so it was swallowed \
                     rather than left unsubmitted. Deliberately NOT pressing a key: with \
                     nothing visibly in the composer, drovr cannot tell a cleared input from a \
                     dialog, and Enter on a dialog accepts its highlighted option on your \
                     behalf (claude's \"New MCP server\" approval reports `idle`, not \
                     `blocked`, so the readiness gate cannot rule it out). Read the pane, \
                     clear whatever is on it, then re-send: {attach}",
                    attach = attach_command(&run.name),
                )
            }
        };
        return Err(send_failure(
            run,
            phase,
            reopened,
            io::ErrorKind::TimedOut,
            &why,
        ));
    }

    // The payload is in the composer, not a menu, so Enter can only submit it.
    h.agent_send_keys(&pane_id, &["enter".to_string()])
        .map_err(|e| {
            send_failure(
                run,
                phase,
                reopened,
                e.kind(),
                &format!(
                    "the seed landed in the agent's composer but the submit keystroke could \
                     not be sent ({e})"
                ),
            )
        })?;
    let nudged = h
        .agent_wait_started(&pane_id, confirm_timeout)
        .map_err(|e| {
            send_failure(
                run,
                phase,
                reopened,
                e.kind(),
                &format!(
                    "the seed landed in the agent's composer and was nudged, but herdr could \
                     not be asked whether it took ({e})"
                ),
            )
        })?;
    if nudged == PromptOutcome::Started {
        return Ok(());
    }

    Err(send_failure(
        run,
        phase,
        reopened,
        io::ErrorKind::TimedOut,
        &format!(
            "the seed landed in the agent's composer but would not submit — it was still \
             unsent after a follow-up Enter and {secs}s. Submit it by hand: {attach}",
            secs = confirm_timeout.as_secs(),
            attach = attach_command(&run.name),
        ),
    ))
}

/// Mark a phase complete by dropping its completion marker. Run BY the phase
/// agent itself as its final action (via `drovr phase done`), NOT by the
/// orchestrator — it is the only reliable "the agent finished" signal, since
/// herdr's `idle` status also fires while an agent is merely parked awaiting a
/// subagent. Writing a marker file (rather than mutating `state.json`) keeps
/// the orchestrator the sole writer of run state.
pub fn phase_done(run: &RunState, phase: &str) -> io::Result<PathBuf> {
    require_phase_name(phase)?;
    // `find_phase` (not `find_phase_idx`) so a reviewer phase living only in
    // `review_phases` can drop its marker: `drovr phase done <run>
    // review:<task>:<iter>:<angle>` is run by the reviewer agent itself.
    run.find_phase(phase).ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, format!("phase not found: {phase}"))
    })?;

    // Self-authored handoff contract: a PIPELINE phase (one in `run.phases`) must
    // have authored a non-empty `<phase>-HANDOFF.md` before it may signal done —
    // the handoff and the done marker are one atomic completion step, so a phase
    // can never be marked done without the briefing the next phase inherits.
    // Reviewer phases (only in `review_phases`) author no handoff and are exempt.
    if run.phases.iter().any(|p| p.name == phase) {
        let handoff = run_dir(&run.name).join(format!("{phase}-HANDOFF.md"));
        // A read failure is NOT "missing or empty": permissions or IO on an existing
        // handoff is a different problem with a different remedy, and reporting it as
        // missing sends the agent to rewrite a file that is already there.
        let contents = match std::fs::read_to_string(&handoff) {
            Ok(c) => c,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                return Err(io::Error::new(
                    e.kind(),
                    format!(
                        "phase '{phase}' cannot signal done: its handoff {} exists but could \
                         not be read: {e}",
                        handoff.display()
                    ),
                ));
            }
        };
        if contents.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "phase '{phase}' cannot signal done: its handoff {} is missing or empty. \
                     As your final action, author {phase}-HANDOFF.md (the 7-section handoff, \
                     git pointers included) into the run dir, THEN run `drovr phase done`.",
                    handoff.display()
                ),
            ));
        }
        // A scaffolded handoff whose sections have no body is the empty form wearing the
        // shape of a handoff: non-empty by every check, carrying nothing the next phase can
        // inherit. `drovr handoff-scaffold` writes one placeholder per section; what the
        // gate asks is whether each section has substance, not whether that word is gone.
        match scan_handoff(&contents) {
            HandoffShape::Untouched { .. } => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "phase '{phase}' cannot signal done: its handoff {} contains nothing \
                         you wrote — every line is still drovr's scaffold. Write the seven \
                         sections from your own context (nothing else will); if one genuinely \
                         has nothing, say so in it (\"None.\"). THEN run `drovr phase done`.",
                        handoff.display()
                    ),
                ));
            }
            HandoffShape::Placeholders(sections) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "phase '{phase}' cannot signal done: its handoff {} still has the \
                         scaffold's `TODO` in {}. Replace {} with what you actually did; if a \
                         section genuinely has nothing, say so in it (\"None.\"). THEN run \
                         `drovr phase done`.",
                        handoff.display(),
                        sections.join(", "),
                        if sections.len() == 1 { "it" } else { "them" },
                    ),
                ));
            }
            HandoffShape::Complete => {}
        }
    }

    let marker = done_marker(&run.name, phase);
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Stamp the marker with the pass token this agent was launched under, so
    // `phase_wait` can tell "the agent I am waiting on finished" from "some
    // earlier agent for this phase, still alive in the reused pane, finished".
    //
    // Written ATOMICALLY (temp + rename), like `RunState::save`: a plain
    // `fs::write` truncates in place, and `phase_wait` polls this file from
    // another process every 500 ms. A poll landing in the truncate window would
    // read an EMPTY token — which, before this was atomic, was treated as
    // "untokenized, accept" and so completed the wrong pass.
    //
    // The surrounding single quotes are stripped defensively: the token reaches
    // the agent through `herdr pane run`'s command string, and if herdr ever
    // hands the argument to a shell that does NOT strip them, the value would
    // arrive as `'abc-1'`. Our tokens never contain a quote, so this is free.
    let token = std::env::var(PASS_ENV).unwrap_or_default();
    let token = token.trim().trim_matches('\'');
    // A marker this process cannot have matched to the running pass must not be
    // written with an exit 0: that tells the AGENT it finished while the driver
    // silently waits out a full timeout. The check is exactly the rule
    // [`marker_completes_pass`] enforces on READ, applied to the token about to be
    // written — write-side and read-side must not be able to drift apart, because
    // any gap between them is a marker on disk that no wait will ever accept.
    // Three cases:
    //   * a tokened phase, no $DROVR_PASS at all (run from outside the pane, or a
    //     pre-token build);
    //   * a tokened phase, a token that is not the one currently recorded — the
    //     routine case this whole mechanism exists for: `phase_start` re-entered
    //     the phase while THIS agent, holding the old token, was still alive;
    //   * an UNTOKENED phase and a token in hand. The mixed-era case: the phase was
    //     started by a build that mints no tokens, while this shell exports a
    //     $DROVR_PASS from somewhere else. A tokened marker against an untokened
    //     phase is an inconsistency the read side rejects outright, so writing one
    //     is a guaranteed hang.
    let expected = run.find_phase(phase).and_then(|p| p.pass.as_ref());
    if !marker_completes_pass(token, expected) {
        // The remedy differs per case, and it is the whole value of the message:
        // for a tokened phase the way out is to SUPPLY the right token; for an
        // untokened one it is to DROP the one being held (no token exists to
        // supply, and inventing one would just fail the read side differently).
        let held = match expected {
            Some(_) if token.is_empty() => format!("${PASS_ENV} is not set"),
            Some(want) => {
                format!("${PASS_ENV} is '{token}', but this phase is now running pass '{want}'")
            }
            None => format!(
                "${PASS_ENV} is '{token}', but this phase has no pass token at all — it was \
                 started by a drovr build that does not mint them, so a tokened marker \
                 against it is an inconsistency `phase wait` refuses"
            ),
        };
        let remedy = phase_done_command(&run.name, phase, expected);
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "phase '{phase}' cannot signal done: {held}, so this marker could never be \
                 matched to the running pass. `drovr phase done` is meant to be run BY the \
                 phase agent, from inside its own pane. If this phase was restarted, the \
                 agent that should signal it is the one started most recently. To complete \
                 it deliberately:\n    {remedy}",
            ),
        ));
    }

    // Unique per writer: the two agents this change exists to distinguish can
    // both be running `phase done` for the same phase, and a shared temp path
    // would have one rename the other's file out from under it.
    let tmp = marker.with_extension(format!(
        "done.tmp.{}.{}",
        std::process::id(),
        new_pass_token()
    ));
    if let Err(e) = std::fs::write(&tmp, token.as_bytes()) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, &marker) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(marker)
}

/// Does a `<phase>.done` marker holding `token` complete the pass identified by
/// `expected`?
///
/// * No `expected` — a phase from a run created before pass tokens, or one this
///   build has never started, whose agent therefore has no `DROVR_PASS` to stamp.
///   Such a phase completes on an UNTOKENIZED marker: emptiness is exactly the
///   evidence that the writer was token-less too. A TOKENED marker against it is
///   an inconsistency (state that once held a token has been lost) and is
///   rejected — which is what keeps this bounded rather than a general fail-open.
/// * Otherwise the tokens must match EXACTLY — an empty or mismatched token does
///   not complete the phase. This is the load-bearing case: the previous pass's
///   agent holds ITS token in an environment fixed at exec time, so a marker it
///   recreates after `phase_start`'s sweep is provably rejected.
///
/// Empty is deliberately NOT accepted for a tokened phase, even though that means
/// `drovr phase done` run from outside the pane cannot complete one. Accepting it
/// would reopen the race twice over: through `fs::write`'s truncate window (hence
/// the atomic marker write in [`phase_done`]), and for an agent launched by a
/// PREVIOUS BUILD of drovr, which carries no token at all — a run re-entered
/// across an upgrade would falsely complete off its old agent. The documented
/// flow has the agent run `phase done` from inside its own pane, where the token
/// is always present.
/// Whether `<phase>.done` on disk completes the CURRENT pass of `phase` — the
/// same question [`phase_wait`] asks of a pipeline phase, for the callers that
/// poll a marker themselves.
///
/// The one such caller is `code_review_run`'s panel loop, which is
/// `review_phases`-aware and never goes through `phase_wait`. It used to ask
/// `done_marker(..).exists()`, which is a question about the FILE rather than
/// about this pass: a reviewer name is reused when a resume respawns an angle in
/// place, so the reviewer being replaced could complete its replacement with the
/// marker it left behind. [`spawn_reviewer`] sweeps that marker and this checks
/// the token, which are the two halves of the same fix — the sweep is the first
/// line of defence, the token is what makes a marker attributable when the sweep
/// is not reached (a reviewer that was NOT respawned, whose previous pass's
/// agent is still alive in its pane).
///
/// **An unreadable marker answers `false`, deliberately and silently.** It is
/// not evidence, and the alternatives are worse: failing the pass over a
/// bookkeeping read, or a diagnostic inside a 500ms poll loop. It also keeps
/// this consistent with the call beside it — `delivered_review` treats a
/// findings file it cannot read as "nothing delivered yet" for the same reason —
/// so both answers on that line degrade the same way, toward waiting. The panel
/// then times out, which is resumable. `phase_wait` does report it, because a
/// pipeline phase has no such fallback.
pub(crate) fn marker_completes_current_pass(run_name: &str, phase: &Phase) -> bool {
    match std::fs::read_to_string(done_marker(run_name, &phase.name)) {
        Ok(token) => marker_completes_pass(token.trim(), phase.pass.as_ref()),
        Err(_) => false,
    }
}

fn marker_completes_pass(token: &str, expected: Option<&PassToken>) -> bool {
    match expected {
        // Legacy phase: no token was ever minted for it, so its agent had no
        // `$DROVR_PASS` either and can only have written an EMPTY marker. Requiring
        // emptiness is what makes this structural rather than merely operational:
        // a tokened marker against an untokened phase is an inconsistency (it means
        // some pass wrote state we then lost), and is rejected rather than trusted.
        None => token.is_empty(),
        Some(want) => want.matches_marker(token),
    }
}

/// The outcome of `phase_wait`. Maps 1:1 to a `drovr phase wait` exit code (see
/// `main.rs`): `Done` = 0, `TimedOut` = 2, `Blocked` = 4, `Superseded` = 5. (An
/// io error is exit 1, surfaced via the `Err` arm, not this enum.)
#[derive(Debug, PartialEq, Eq)]
pub enum PhaseWaitOutcome {
    /// The completion marker appeared — the phase agent ran `drovr phase done`.
    Done,
    /// `timeout_ms` elapsed with neither a marker nor a `blocked` status.
    TimedOut,
    /// herdr reported the phase pane's `agent_status` as `blocked` — the agent
    /// hit a Claude Code safety/permission prompt with no human at the pane.
    Blocked,
    /// This wait was SUPERSEDED: while it ran, another pass re-entered the phase
    /// (a `phase start`, minting a new token), so the pass this wait was watching
    /// no longer exists.
    ///
    /// Detected at exactly two points, both by classifying the entry snapshot's
    /// pass against a freshly loaded one with [`PassDrift`] (a token that VANISHED
    /// is not a re-entry and never lands here): when a marker matching the OLD pass
    /// lands (the rare ordering — the superseded agent signalled done), and when
    /// the wait runs out of time (the common one — it signalled nothing). What is
    /// deliberately NOT detected is a re-entry via `phase send`, which leaves the
    /// token unchanged and is therefore structurally invisible to a token
    /// comparison; it is still unsolved, and needs a monotonic re-entry counter
    /// beside the pass token.
    ///
    /// Deliberately NOT `TimedOut`, which it used to be reported as. The two are
    /// opposite verdicts about the same phase — "another pass took over, and it is
    /// the one to follow now" versus "the agent I am waiting on is not
    /// progressing" — and nothing but log scraping could tell them apart. Pane
    /// teardown is keyed off verdicts like this one, and the pane here belongs to
    /// the LIVE re-entry: a caller must be able to see that without parsing
    /// prose.
    ///
    /// Like the `Done` path, this outcome adopts the freshly loaded run state
    /// (`*run = fresh`), so a caller that saves after waiting writes the
    /// re-entry's state rather than restoring the superseded pass.
    Superseded,
}

/// Poll the filesystem for the phase's completion marker (dropped by the phase
/// agent via `drovr phase done`) AND the phase pane's herdr `agent_status` until
/// the marker appears, the pane goes `blocked`, or `timeout_ms` elapses. Marks
/// the phase Done (and saves) when the marker is found; leaves it Running on
/// timeout or block. A wait whose pass another `phase start` has superseded
/// meanwhile returns [`PhaseWaitOutcome::Superseded`] and writes nothing — checked
/// both when a matching marker lands and when the wait runs out of time, so a
/// re-entry does not have to produce a marker to be reported as one.
///
/// herdr status is consulted ONLY to catch `blocked` early — a proactive signal
/// that the agent hit a safety/permission prompt and will otherwise hang until the
/// full timeout. Every other status (`idle`, `working`, `done`, `unknown`) is
/// ignored: `idle` in particular is NOT a completion signal (it also fires when an
/// agent is parked awaiting its own subagent), so only the `.done` marker counts
/// as done. The status query is read-only (`pane get`) and never moves focus, so
/// polling it each iteration does not disturb the user.
///
/// Evidence rule — the reason there is no `status == Done` short-circuit here:
/// a phase is complete iff its `<phase>.done` marker exists AND carries the
/// CURRENT pass's token. The recorded `Done` status is a cache of that evidence,
/// never a substitute for it. Deriving the verdict from the marker every time is
/// what makes the whole family of "state was persisted but the marker was
/// destroyed (or vice versa)" failures fail closed: an interrupted `phase_start`
/// or `phase_send` can leave a stale `Done` on disk, and short-circuiting on it
/// would report success for a phase with no agent running. A `Done` phase is
/// exactly what the next launch reaps, so that would close a live agent's pane.
///
/// Consequently the marker is NOT consumed on success — it IS the evidence, and
/// keeping it is what makes a repeated wait idempotent without a status
/// short-circuit. A later pass cannot be fooled by a leftover marker: it holds a
/// different token, and both re-entry paths (`phase_start`, `phase_send`) sweep it.
///
/// Source-of-truth note: this stays bound to `run.phases` ONLY (via
/// `find_phase_idx`). Reviewer phases live in `review_phases` and are NEVER waited
/// on here — the code-review orchestrator runs its own marker poll loop and updates
/// `review_phases` status directly. Do not switch this to `find_phase`.
pub fn phase_wait<H: Herdr>(
    h: &H,
    run: &mut RunState,
    phase: &str,
    timeout_ms: u64,
) -> io::Result<PhaseWaitOutcome> {
    require_phase_name(phase)?;
    let idx = find_phase_idx(run, phase).ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, format!("phase not found: {phase}"))
    })?;
    // NOTE: there is deliberately NO `status == Done` short-circuit, and the
    // marker is deliberately NOT consumed on success. See the doc comment above —
    // the marker is the evidence, the status is only a cache of it.
    let expected_pass = run.phases[idx].pass.clone();
    let marker = done_marker(&run.name, phase);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut mismatch_reported = false;
    let mut read_error_reported = false;
    // Gated like the other two: the marker keeps matching, so this branch is
    // re-entered on every poll until the deadline.
    let mut token_lost_reported = false;
    loop {
        // POLL FIRST, before any branch that can return. Exactly one poll per
        // iteration, and it captures the session on the way past.
        //
        // It used to sit after the marker branch, on the reasoning that no
        // bookkeeping write should wedge in front of a verdict. That reasoning
        // was wrong in the direction that matters: the marker branch RETURNS
        // (`Done`, `Superseded`), so a phase whose marker was already on disk at
        // the first iteration was never polled at all — and its session is
        // readable only while its agent is alive. Capture must not be reachable
        // only on the slow path.
        //
        // Nothing is wedged in front of anything: capture writes through freshly
        // loaded state, so the marker branch below re-reads it rather than
        // racing it, and the `Done` path's `*run = fresh` picks it up.
        let status = poll_phase_pane(h, run, phase).and_then(|info| info.agent_status);
        match std::fs::read_to_string(&marker) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                // Not "no marker yet" — the evidence is there and unreadable.
                // Silently polling on would report a timeout for a phase that may
                // well have finished, so say it once.
                if !read_error_reported {
                    read_error_reported = true;
                    eprintln!(
                        "drovr: phase '{phase}' has a completion marker that cannot be read \
                         ({}): {e}. Waiting on, but this will time out until it is fixed.",
                        marker.display()
                    );
                }
            }
            Err(_) => {}
            Ok(token) => {
                let token = token.trim();
            if marker_completes_pass(token, expected_pass.as_ref()) {
                // Before committing, check this wait has not been SUPERSEDED.
                // `expected_pass` was snapshotted at entry and a wait can run for
                // an hour; if a `phase start` re-entered the phase meanwhile, this
                // process waits on a pass that no longer exists and the marker it
                // matched belongs to that dead pass. Completing would report a
                // false `Done` for the live agent AND clobber the re-entry.
                //
                // Fails CLOSED: a load error aborts the wait rather than skipping
                // the check. A completion that cannot be verified is exactly what
                // must never be reported — a `Done` phase is what the next launch
                // reaps, so an unverifiable `Done` closes a live agent's pane.
                let mut fresh = RunState::load(&run.name).map_err(|e| {
                    io::Error::new(
                        e.kind(),
                        format!(
                            "phase '{phase}' looks complete, but its run state could not be \
                             re-read to confirm this wait was not superseded: {e}. Refusing to \
                             report done on unverified state."
                        ),
                    )
                })?;
                // Resolve against `phases` only — the same binding `phase_wait`
                // uses at entry, and for the same reason: a `review_phases` entry
                // must never answer for a pipeline phase.
                //
                // A phase MISSING from the fresh state is its own failure, kept
                // distinct from "present with no token". Flattening the two (with
                // `and_then`) makes a vanished legacy phase compare `None == None`,
                // pass the guard, write nothing, and still return `Done`.
                let Some(i) = fresh.phases.iter().position(|p| p.name == phase) else {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!(
                            "phase '{phase}' looks complete, but it is no longer present in \
                             the run state. Refusing to report done on state that cannot \
                             account for it."
                        ),
                    ));
                };
                match PassDrift::between(fresh.phases[i].pass.as_ref(), expected_pass.as_ref()) {
                    PassDrift::Superseded => {
                        eprintln!(
                            "drovr: phase '{phase}' was re-entered while this wait was running \
                             (it was waiting on pass {was}, the phase is now on pass {now}) — the \
                             completion it saw belongs to the superseded pass. Not marking done.",
                            was = expected_pass.as_ref().map_or("<none>", |p| p.as_str()),
                            now = fresh.phases[i].pass.as_ref().map_or("<none>", |p| p.as_str()),
                        );
                        // Adopt the fresh state, exactly as the `Done` path below
                        // does and for the same reason: a caller that saves after
                        // waiting (reaping records a reap that way) would otherwise
                        // write this waiter's hour-old snapshot back, restoring the
                        // superseded pass token and undoing the re-entry that
                        // superseded it. Nothing is written HERE — the re-entry
                        // already persisted everything, and this wait has no verdict
                        // of its own to record.
                        *run = fresh;
                        return Ok(PhaseWaitOutcome::Superseded);
                    }
                    // The token this waiter (and its live agent) hold has vanished
                    // from disk. NOT a completion — the fresh state cannot account
                    // for the token the marker carries — and NOT a supersession
                    // either, so it must not be reported as one. Keep waiting: the
                    // caller's snapshot still holds the token, which is what makes
                    // the documented recovery possible.
                    PassDrift::TokenLost => {
                        if !token_lost_reported {
                            token_lost_reported = true;
                            eprintln!("{}", token_lost_message(phase, &run.name));
                        }
                    }
                    PassDrift::Same => {
                        // Commit onto the FRESHLY loaded state, not the snapshot
                        // taken at entry: writing an hour-old whole-state copy back
                        // would silently undo everything else that happened to the
                        // run meanwhile.
                        fresh.phases[i].status = PhaseStatus::Done;
                        fresh.save()?;
                        // Adopt the fresh state wholesale rather than patching one
                        // field into the caller's stale snapshot. A caller that
                        // saves after waiting (reaping records the reap this way)
                        // would otherwise write that snapshot back and undo exactly
                        // what this block prevents.
                        *run = fresh;
                        // The marker is NOT removed. It is the durable evidence
                        // that this pass finished, it makes a repeated wait
                        // idempotent with no status short-circuit, and a later pass
                        // cannot be fooled by it — that pass has a different token,
                        // and both re-entry paths sweep it.
                        return Ok(PhaseWaitOutcome::Done);
                    }
                }
            } else {
                // A marker from a DIFFERENT pass: the previous pass's agent is still
                // alive in the reused pane and signalled done again.
                //
                // IGNORE it — do NOT delete it. A `phase wait` left over from an
                // earlier pass holds that pass's token in memory and never re-reads
                // state.json; if mismatches were unlinked, that stale waiter would
                // delete the CURRENT pass's marker the moment it landed, and the real
                // waiter would then time out on a phase that actually completed. The
                // current agent's own marker overwrites this path when it lands, and
                // both re-entry paths sweep it, so ignoring is sufficient.
                //
                // Announce it once: a silently-rejected marker is indistinguishable
                // from "the agent never finished", and this is the one signal that
                // tells a human the token transport (or a stale agent) is the problem.
                if !mismatch_reported {
                    mismatch_reported = true;
                    eprintln!(
                        "{}",
                        marker_mismatch_message(
                            phase,
                            &run.name,
                            token,
                            expected_pass.as_ref()
                        )
                    );
                }
            }
            }
        }
        // ONE poll per iteration, answering both questions below.
        //
        // It sits AFTER the marker branch on purpose: every path that branch can
        // return on (`Done`, `Superseded`, an unverifiable completion) is a
        // verdict about this wait, and none of them wants a bookkeeping write
        // wedged in front of it. Reached on every iteration that does not return,
        // including the `TokenLost` one — a pass token vanishing from drovr's own
        // state says nothing about whether the agent is alive, so its session
        // stays captured (and keeps being captured).
        // Proactively catch a blocked pane so the driver is signalled immediately
        // instead of hanging until the wait's full timeout. Only `blocked` short-
        // circuits; every other status keeps waiting for the marker.
        //
        // Read from the poll taken at the top of this iteration — deliberately
        // AFTER the marker branch, so a phase that is genuinely complete still
        // reports `Done` even if its pane happens to read `blocked`. Moving the
        // poll earlier changed when the pane is read, not what wins.
        if status == Some(AgentStatus::Blocked) {
            return Ok(PhaseWaitOutcome::Blocked);
        }
        let now = Instant::now();
        if now >= deadline {
            // Before reporting "the agent I am waiting on is not progressing",
            // ask whether that agent is still the one this phase is running.
            // Supersession is only visible through the marker when the dead pass
            // happens to signal done — which is the RARE ordering. The common one
            // is the driver's: a `phase start` re-entered the phase and the
            // superseded agent never signalled anything, so this waiter simply
            // sits there and used to report a plain timeout for a phase that is
            // healthy under a newer pass. One state read, once, at the end.
            return Ok(timed_out_or_superseded(
                run,
                phase,
                expected_pass.as_ref(),
                token_lost_reported,
            ));
        }
        thread::sleep(POLL_INTERVAL.min(deadline - now));
    }
}

/// What happened to a phase's pass token while a wait was running, compared
/// against the token that wait snapshotted at entry.
///
/// The distinction that matters is between "a NEWER pass exists" and "the token
/// went away". A bare `!=` conflates them, and the conflation is not theoretical:
/// task 1's handoff §5.6 records that an older `drovr` on `PATH` drops the `pass`
/// field on any save (serde omits what its struct does not know), taking a phase
/// from `Some(x)` to `None` with no re-entry whatsoever. Reporting that as
/// supersession tells the driver "nothing is wrong, re-run the wait" about a phase
/// that will now hang forever — an untokened phase only accepts an EMPTY marker,
/// and its live agent still stamps the token it was launched with. A misleading
/// verdict is worse than the honest timeout it replaced.
#[derive(Debug, PartialEq, Eq)]
enum PassDrift {
    /// Same pass. The wait is still watching the pass it started on.
    Same,
    /// A NEWER pass exists: either a different token, or a token where this wait
    /// snapshotted none (`phase_start` always mints `Some`, so a legacy phase
    /// acquiring a token is a genuine re-entry by a token-minting build).
    Superseded,
    /// The token this wait holds has VANISHED from disk. Not a re-entry —
    /// corruption or a lossy writer. Stays a timeout, loudly.
    TokenLost,
}

impl PassDrift {
    fn between(fresh: Option<&PassToken>, expected: Option<&PassToken>) -> PassDrift {
        match (fresh, expected) {
            (None, Some(_)) => PassDrift::TokenLost,
            (Some(now), Some(was)) if now != was => PassDrift::Superseded,
            (Some(_), None) => PassDrift::Superseded,
            _ => PassDrift::Same,
        }
    }
}

/// The diagnostic for a `<phase>.done` whose token belongs to a different pass.
/// A function rather than an inline `eprintln!` so the command it suggests can be
/// tested: this suite captures no output (see the handoff), and this string is
/// the one that carries a run name and a token into something a human pastes.
fn marker_mismatch_message(
    phase: &str,
    run_name: &str,
    marker_token: &str,
    expected: Option<&PassToken>,
) -> String {
    let awaiting = expected.map_or("<none>", |p| p.as_str());
    format!(
        "drovr: phase '{phase}' has a completion marker from a different pass \
         (marker token {marker_token:?}, awaiting {awaiting:?}) — ignoring it and \
         continuing to wait for this pass's agent. If the phase really did \
         finish and you want to accept this, re-signal it deliberately: {}",
        phase_done_command(run_name, phase, expected)
    )
}

/// The diagnostic for [`PassDrift::TokenLost`]. Names the recovery, because this
/// state does not heal on its own: every later wait sees the same thing.
fn token_lost_message(phase: &str, run_name: &str) -> String {
    format!(
        "drovr: phase '{phase}' has lost its pass token from {run_name}'s state.json while this \
         wait was running — the phase now records NO token, but its agent still holds one, so no \
         marker it writes can be accepted. This is not a re-entry (a re-entry mints a token, it \
         does not remove one); the usual cause is an older drovr binary re-saving the run. \
         Recover by re-entering the phase deliberately: drovr phase start {} {}",
        shell_single_quote(run_name),
        shell_single_quote(phase)
    )
}

/// Classify a wait that ran out of time: did it time out, or was it SUPERSEDED
/// while it ran?
///
/// Unlike the marker path's guard, this one fails OPEN — to `TimedOut`. There the
/// question is "may I report a completion I cannot verify" and the answer must be
/// no; here the conservative answer already IS `TimedOut` ("keep waiting / go
/// look"), so a state file that cannot be read must not be turned into an error
/// that aborts an otherwise honest timeout.
///
/// Adopts the fresh state on the superseded path for the same reason the `Done`
/// path does: the caller may save after waiting, and this waiter's snapshot still
/// names the pass that no longer exists.
///
/// `token_lost_reported` carries the loop's gate in, so a wait that already
/// explained a vanished token mid-poll does not repeat itself at the deadline:
/// one message per event, like every other diagnostic here.
fn timed_out_or_superseded(
    run: &mut RunState,
    phase: &str,
    expected: Option<&PassToken>,
    token_lost_reported: bool,
) -> PhaseWaitOutcome {
    let fresh = match RunState::load(&run.name) {
        Ok(fresh) => fresh,
        Err(e) => {
            eprintln!(
                "drovr: phase '{phase}' timed out, and its run state could not be re-read to \
                 check whether this wait had been superseded ({e}). Reporting the timeout."
            );
            return PhaseWaitOutcome::TimedOut;
        }
    };
    // `phases` only, like every other resolution in this function: a
    // `review_phases` entry must never answer for a pipeline phase. A phase that
    // has VANISHED is not evidence of a re-entry, so it stays a timeout — the
    // marker path errors on that case because it is about to report a completion,
    // and this one is not.
    let Some(i) = fresh.phases.iter().position(|p| p.name == phase) else {
        return PhaseWaitOutcome::TimedOut;
    };
    match PassDrift::between(fresh.phases[i].pass.as_ref(), expected) {
        PassDrift::Same => PhaseWaitOutcome::TimedOut,
        // A vanished token is not a re-entry, so it must not be reported as one —
        // see [`PassDrift`]. Reported here because a timeout is otherwise the one
        // outcome that explains nothing, and this state repeats on every wait.
        PassDrift::TokenLost => {
            if !token_lost_reported {
                eprintln!("{}", token_lost_message(phase, &run.name));
            }
            PhaseWaitOutcome::TimedOut
        }
        PassDrift::Superseded => {
            eprintln!(
                "drovr: phase '{phase}' was re-entered while this wait was running (it was \
                 waiting on pass {was}, the phase is now on pass {now}) — the pass it was \
                 watching is gone.",
                was = expected.map_or("<none>", |p| p.as_str()),
                now = fresh.phases[i].pass.as_ref().map_or("<none>", |p| p.as_str()),
            );
            *run = fresh;
            PhaseWaitOutcome::Superseded
        }
    }
}

// ---------------------------------------------------------------------------
// Liveness net — detect a phase pane parked on a first-run prompt
// ---------------------------------------------------------------------------
//
// A freshly-spawned interactive `claude` can PARK on a prompt with no human to
// answer it — a first-run upsell, a workspace-trust dialog, a future onboarding
// step — and then it never reaches its composer, never runs `drovr phase done`,
// and `phase wait` just times out with no explanation. The renderer upsell is
// handled at the source (`CLAUDE_CODE_NO_FLICKER=1`, see herdr::spawn_env_flags),
// but other prompts are unforeseeable, so this net catches the general case:
// when a wait times out, read the pane once and, if it matches a known
// "waiting on a prompt" signature, surface a clear, actionable diagnostic
// (naming the run/phase, quoting the pane, suggesting `drovr attach`) instead of
// a silent timeout.
//
// Design choices:
//   * SURFACE, never auto-dismiss. Guessing a keypress to clear an unknown
//     prompt is fragile — a wrong guess corrupts the agent's input — so we tell
//     the human exactly where to look rather than gambling.
//   * Read-only. `agent read` never moves focus (it is the same call `phase
//     compress` uses without any focus capture), so this is focus-safe by
//     construction and needs no capture/restore.
//   * No busy-poll. This runs exactly once, on the wait's timeout — not in a
//     loop.

/// Substrings that mark a pane as parked on an interactive prompt awaiting a
/// human. Kept deliberately specific to avoid matching normal agent output.
/// Matching is case-insensitive.
const STUCK_PROMPT_SIGNATURES: &[&str] = &[
    // claude's first-run fullscreen-renderer upsell (belt-and-suspenders: it is
    // suppressed at spawn via CLAUDE_CODE_NO_FLICKER, but a future variant might
    // slip through).
    "try the new fullscreen",
    // Generic numbered-choice prompt cursor, e.g. "❯ 1. Yes".
    "❯ 1.",
    // Workspace / directory trust dialog.
    "do you trust",
    // Common confirmation affordances.
    "enter to confirm",
    "press enter to continue",
    "1. yes",
];

/// If `pane` looks like it is parked on an interactive prompt awaiting a human,
/// return the signature that matched (for the diagnostic); otherwise `None`.
/// Pure and case-insensitive so it is trivially unit-testable.
fn detect_stuck_prompt(pane: &str) -> Option<&'static str> {
    let haystack = pane.to_lowercase();
    STUCK_PROMPT_SIGNATURES
        .iter()
        .copied()
        .find(|sig| haystack.contains(&sig.to_lowercase()))
}

/// The last `n` non-blank lines of `pane`, joined — a compact snippet to quote
/// in the diagnostic so the human can recognize the prompt without attaching.
///
/// Shared with [`crate::blocked`], which quotes the same tail into the browser
/// badge and `drovr watch`, so every surface that shows a human "what is your
/// agent stuck on" shows them the same lines.
pub(crate) fn tail_snippet(pane: &str, n: usize) -> String {
    let lines: Vec<&str> = pane
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// Read the phase pane once and, if it is parked on a known interactive prompt,
/// return a clear, actionable diagnostic (naming the run/phase, quoting the
/// pane tail, suggesting `drovr attach`). Returns `None` when the pane is not
/// recognizably stuck (it may just be mid-work) or has no pane_id yet.
///
/// Resolves the pane via `find_phase` (both `phases` AND `review_phases`) so a
/// `TimedOut` on `drovr phase send <run> review:...` still gets a pane snapshot,
/// not just the bare error text.
///
/// Read-only and focus-safe: `agent_read` never moves focus, so no capture /
/// restore is needed. Intended to be called ONCE on a timeout, not in a poll
/// loop. A failed pane read is swallowed (returns `None`) — a best-effort
/// diagnostic must never mask the underlying timeout with a new error.
pub fn diagnose_stuck_phase<H: Herdr>(h: &H, run: &RunState, phase: &str) -> Option<String> {
    let pane_id = run.find_phase(phase)?.pane_id().map(str::to_owned)?;
    let pane = h.agent_read(&pane_id).ok()?;
    let matched = detect_stuck_prompt(&pane)?;
    let snippet = tail_snippet(&pane, 6);
    Some(format!(
        "phase '{phase}' of run '{run_name}' appears STUCK on an interactive prompt \
         (matched \"{matched}\") rather than working — it will never signal `drovr phase done`, \
         so `phase wait` timed out.\n\
         Pane {pane_id}:\n{snippet}\n\
         Attach to answer the prompt: {attach}",
        run_name = run.name,
        attach = attach_command(&run.name),
    ))
}

// ---------------------------------------------------------------------------
// Blocked-phase triage — classify a safety/permission prompt and escalate
// ---------------------------------------------------------------------------
//
// When herdr reports a phase pane as `blocked`, the agent hit a Claude Code
// safety/permission prompt (a "Dangerous rm operation … 1. Yes / 2. No"
// confirmation, a tool-permission dialog, …) with no human at the pane. This
// module classifies what it is blocked on and decides whether to escalate to a
// human or auto-answer:
//
//   * DESTRUCTIVE / dangerous prompts are NEVER auto-answered — a wrong guess on
//     an `rm -rf` / `reset --hard` / force-push confirmation is unrecoverable.
//     We surface a clear diagnostic and let the driver escalate to a human.
//   * ROUTINE, clearly-safe tool-permission prompts on a small allow-list MAY be
//     auto-answered by sending the accept keystroke (conservative + opt-in).
//   * UNKNOWN prompts escalate — when in doubt, ask a human.
//
// The classifier is a PURE function (`classify_blocked_prompt`) so it is trivially
// unit-tested. Destructive is checked FIRST and wins over routine, so a prompt
// that mentions both (e.g. a Bash permission dialog whose command is `rm -rf`)
// escalates rather than being auto-answered.

/// How a `blocked` phase pane is triaged.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BlockedClass {
    /// A destructive / dangerous confirmation (rm, delete, reset --hard, force,
    /// overwrite, …). NEVER auto-answered — always escalate to a human.
    Destructive,
    /// An ordinary, clearly-safe tool-permission prompt on the allow-list. May be
    /// auto-answered with the accept keystroke.
    Routine,
    /// A prompt that is neither recognizably destructive nor on the routine
    /// allow-list. Escalate to a human rather than guessing.
    Unknown,
}

impl BlockedClass {
    /// Whether a HUMAN has to answer this prompt, as opposed to a driver's
    /// `phase wait` answering it.
    ///
    /// The escalation rule lives here, on the enum that decides it, so a caller
    /// holding only a class never has to re-encode `!matches!(Routine)` — a new
    /// variant then changes the policy in one place rather than in every site
    /// that spelled the match out.
    ///
    /// It is the same line [`triage_blocked_phase`] draws: destructive and
    /// unknown prompts are never auto-answered, so nothing clears them until a
    /// person acts.
    pub fn needs_human(self) -> bool {
        !matches!(self, BlockedClass::Routine)
    }

    /// The wire name, as the review server's JSON and the CLI's watcher output
    /// spell it. One spelling for both so a badge and a log line about the same
    /// pane never disagree about what it is blocked on.
    pub fn as_str(self) -> &'static str {
        match self {
            BlockedClass::Destructive => "destructive",
            BlockedClass::Routine => "routine",
            BlockedClass::Unknown => "unknown",
        }
    }
}

/// Substrings that mark a blocked pane as a DESTRUCTIVE / dangerous confirmation.
/// These MUST take precedence over the routine allow-list. Matching is
/// case-insensitive. Kept broad on the danger side deliberately: a false positive
/// only means "ask a human", which is always safe.
const DESTRUCTIVE_SIGNATURES: &[&str] = &[
    "dangerous",
    "dangerous rm",
    "rm -rf",
    "delete",
    "reset --hard",
    "force", // covers "force push", "--force", "force overwrite"
    "overwrite",
    "drop table",
    "truncate",
    "destroy",
];

/// Substrings that mark a blocked pane as a ROUTINE, clearly-safe tool-permission
/// prompt eligible for conservative auto-answer. Deliberately small; anything not
/// listed here (and not destructive) is treated as Unknown and escalated.
/// Matching is case-insensitive.
const ROUTINE_SIGNATURES: &[&str] = &[
    "do you want to make this edit",
    "do you want to create",
    "do you want to read",
    "would you like to proceed with reading",
];

/// Classify what a blocked phase pane is waiting on. Pure and case-insensitive so
/// it is trivially unit-testable. Destructive is checked FIRST so a prompt that
/// matches both a danger word and a routine phrase escalates rather than being
/// auto-answered.
pub fn classify_blocked_prompt(pane: &str) -> BlockedClass {
    let haystack = pane.to_lowercase();
    if DESTRUCTIVE_SIGNATURES
        .iter()
        .any(|sig| haystack.contains(&sig.to_lowercase()))
    {
        return BlockedClass::Destructive;
    }
    if ROUTINE_SIGNATURES
        .iter()
        .any(|sig| haystack.contains(&sig.to_lowercase()))
    {
        return BlockedClass::Routine;
    }
    BlockedClass::Unknown
}

/// The result of triaging a blocked phase: the classification, the human-facing
/// diagnostic to print, and whether drovr auto-answered the prompt (and may keep
/// waiting). Only routine, allow-listed prompts are auto-answered; destructive and
/// unknown prompts always escalate (`auto_answered == false`).
#[derive(Debug)]
pub struct BlockedTriage {
    pub class: BlockedClass,
    pub diagnostic: String,
    pub auto_answered: bool,
}

/// The keystroke sent to accept a routine, allow-listed permission prompt.
/// Claude Code's numbered prompts default the highlighted choice to the safe
/// "yes" (option 1); a bare Enter accepts it. We send exactly that and nothing
/// else — never a chosen digit, which could land on the wrong option.
const ACCEPT_KEYSTROKE: &str = "\r";

/// Triage a phase pane that herdr reported as `blocked`: read it once, classify
/// the prompt, and either escalate (destructive/unknown) or conservatively
/// auto-answer (routine allow-list). Returns a `BlockedTriage` with the human
/// diagnostic and whether it auto-answered.
///
/// Read-only up to the auto-answer decision: `agent_read` never moves focus.
/// Auto-answer sends the accept keystroke via `agent_send` (which drovr already
/// uses to seed panes); a failed read or send degrades gracefully to an escalation
/// diagnostic rather than erroring.
pub fn triage_blocked_phase<H: Herdr>(h: &H, run: &RunState, phase: &str) -> BlockedTriage {
    let escalate = |class: BlockedClass, body: String| BlockedTriage {
        class,
        diagnostic: format!(
            "phase '{phase}' of run '{run_name}' is BLOCKED on a Claude Code \
             safety/permission prompt with no human at the pane.\n{body}\n\
             Attach to answer it: {attach}",
            run_name = run.name,
            attach = attach_command(&run.name),
        ),
        auto_answered: false,
    };

    let Some(idx) = find_phase_idx(run, phase) else {
        return escalate(
            BlockedClass::Unknown,
            "(phase has no recorded pane; cannot read the prompt)".into(),
        );
    };
    let Some(pane_id) = run.phases[idx].pane_id().map(str::to_owned) else {
        return escalate(
            BlockedClass::Unknown,
            "(phase has no pane_id; cannot read the prompt)".into(),
        );
    };
    let Ok(pane) = h.agent_read(&pane_id) else {
        return escalate(
            BlockedClass::Unknown,
            format!("(pane {pane_id} blocked, but its contents could not be read)"),
        );
    };
    let snippet = tail_snippet(&pane, 6);
    let class = classify_blocked_prompt(&pane);
    match class {
        BlockedClass::Destructive => escalate(
            BlockedClass::Destructive,
            format!(
                "The prompt looks DESTRUCTIVE — drovr will NOT auto-answer it.\n\
                 Pane {pane_id}:\n{snippet}"
            ),
        ),
        BlockedClass::Unknown => escalate(
            BlockedClass::Unknown,
            format!(
                "The prompt is not on the safe auto-answer allow-list — escalating \
                 rather than guessing.\nPane {pane_id}:\n{snippet}"
            ),
        ),
        BlockedClass::Routine => {
            // Conservative auto-answer: accept the routine, allow-listed prompt.
            // A send failure falls back to escalation so the human still gets a
            // clear signal.
            match h.agent_send(&pane_id, ACCEPT_KEYSTROKE) {
                Ok(()) => BlockedTriage {
                    class: BlockedClass::Routine,
                    diagnostic: format!(
                        "phase '{phase}' of run '{run_name}' blocked on a routine, \
                         allow-listed permission prompt; drovr auto-answered it \
                         (accept) and is continuing.\nPane {pane_id}:\n{snippet}",
                        run_name = run.name,
                    ),
                    auto_answered: true,
                },
                Err(e) => escalate(
                    BlockedClass::Routine,
                    format!(
                        "A routine prompt was detected but auto-answer failed ({e}); \
                         escalating.\nPane {pane_id}:\n{snippet}"
                    ),
                ),
            }
        }
    }
}

/// Read `<phase>-HANDOFF.md`, authored by the finishing phase agent, from the run directory.
pub fn collect(run: &RunState, phase: &str) -> io::Result<String> {
    require_phase_name(phase)?;
    let path: PathBuf = run_dir(&run.name).join(format!("{phase}-HANDOFF.md"));
    std::fs::read_to_string(&path).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("collect({phase}): cannot read {}: {e}", path.display()),
        )
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentLaunch;
    use crate::herdr::FakeHerdr;
    use crate::run::{Phase, PhaseStatus, RunState};
    use crate::test_util::ENV_LOCK;

    fn make_run(name: &str) -> RunState {
        // Caller must hold ENV_LOCK before calling.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", format!("/tmp/drovr-phase-test-{name}"));
        }
        // Start each test from a clean run dir so a stale `.done` marker or
        // state.json from a prior run can't leak across test invocations.
        let _ = std::fs::remove_dir_all(run_dir(name));
        // `phase_done` stamps the pass token into the marker; a value left behind
        // by another test would silently change what `phase_wait` accepts.
        unsafe {
            std::env::remove_var(PASS_ENV);
        }
        RunState {
            name: name.to_owned(),
            task: "test task".into(),
            agent: Some("claude".into()),
            phases: vec![],
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            // `drovr new` always creates a workspace + root shell pane. Every
            // phase gets its own tab; the root pane stays idle, and is here so
            // tests can prove nothing reaches for it.
            workspace: Some("ws-mk".into()),
            root_pane: Some("root-mk".into()),
            project_dir: "/tmp/drovr-proj-test".into(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
        }
    }

    fn make_run_with_workspace(name: &str, ws_id: &str) -> RunState {
        let mut run = make_run(name);
        run.workspace = Some(ws_id.to_owned());
        run.root_pane = Some(format!("{ws_id}:root"));
        run
    }

    // -- workspace recovery ---------------------------------------------------

    /// The live failure this whole area exists for: reaping the last pane in a
    /// run's workspace makes herdr destroy the workspace, `state.json` goes on
    /// naming it, and `phase start` used to die on the raw
    /// `workspace_not_found`. A workspace is disposable infrastructure; the run's
    /// phases, handoffs and commits are not.
    #[test]
    fn phase_start_reprovisions_a_workspace_that_vanished() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("ws-gone-test", "wAG");
        run.phases.push({
                let mut p = Phase::new("plan");
                p.status = PhaseStatus::Done;
                p
            }
            .with_pane("wAG:p1"));
        // The driver closed the last pane; herdr took the workspace with it.
        h.kill_workspace("wAG", ["wAG:root".to_string(), "wAG:p1".to_string()]);

        phase_start(&h, &mut run, "implement", None).expect("a dead workspace must be recoverable");

        let calls = h.calls();
        let created = calls
            .iter()
            .find(|c| c.contains("workspace_create"))
            .unwrap_or_else(|| panic!("a vanished workspace must be re-created: {calls:?}"));
        // The near-miss from the manual recovery: a workspace created without the
        // run's cwd opens in whatever directory herdr defaults to, and briefing an
        // agent there is silent. If drovr owns creation, drovr owns the cwd.
        assert!(
            created.contains("cwd=/tmp/drovr-proj-test"),
            "the replacement workspace must open in the run's project_dir: {created}"
        );
        assert!(
            created.contains("label=drovr:ws-gone-test"),
            "the replacement must be labelled for the run, like `drovr new`'s: {created}"
        );

        assert_ne!(
            run.workspace.as_deref(),
            Some("wAG"),
            "the new workspace id must be recorded, not the dead one"
        );
        let ws = run.workspace.clone().expect("a workspace must be recorded");
        // And the phase really launched into the NEW workspace — recording the id
        // while spawning into the corpse would be the same bug one level down.
        //
        // Asserted on the `tab_create` CALL, not on the shape of the pane id.
        // Phases no longer reuse the workspace root pane (whose id embeds the
        // workspace), so the only place the target workspace appears is the call
        // that opened the tab.
        let pane = run.phases.iter().find(|p| p.name == "implement").unwrap();
        assert!(
            pane.pane_id().is_some(),
            "the phase must have a pane"
        );
        assert!(
            h.calls()
                .iter()
                .any(|c| c.contains(&format!("tab_create workspace={ws}"))),
            "the phase tab must be opened in the new workspace {ws}: {:?}",
            h.calls()
        );
        assert_eq!(pane.status, PhaseStatus::Running);
        // Persisted, not just in memory: the next command loads from disk.
        assert_eq!(RunState::load("ws-gone-test").unwrap().workspace, Some(ws));
    }

    /// Open question 2, pinned. Every pane recorded in the dead workspace is
    /// dangling, and a `Running` phase's agent is gone with its context. Marking
    /// it `Failed` says that out loud; leaving it `Running` would advertise work
    /// nobody is doing.
    #[test]
    fn reprovisioning_fails_the_phases_whose_agents_died_with_the_workspace() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("ws-orphan-test", "wAG");
        run.phases.push({
                let mut p = Phase::new("plan");
                p.status = PhaseStatus::Done;
                p
            }
            .with_pane("wAG:p1"));
        run.phases.push({
                let mut p = Phase::new("brainstorm");
                p.status = PhaseStatus::Running;
                p
            }
            .with_pane("wAG:p2"));
        run.review_phases.push({
                let mut p = Phase::new("review:plan:1:correctness");
                p.status = PhaseStatus::Running;
                p
            }
            .with_pane("wAG:p3"));
        run.retire_pane("wAG:p4");
        h.kill_workspace("wAG", ["wAG:root".to_string()]);

        phase_start(&h, &mut run, "implement", None).unwrap();

        let plan = run.phases.iter().find(|p| p.name == "plan").unwrap();
        assert_eq!(
            plan.status,
            PhaseStatus::Done,
            "a finished phase does not care that its pane is gone"
        );
        assert!(
            plan.pane_id().is_none(),
            "but its pane id names a pane that no longer exists and must be dropped"
        );

        let brainstorm = run.phases.iter().find(|p| p.name == "brainstorm").unwrap();
        assert_eq!(
            brainstorm.status,
            PhaseStatus::Failed,
            "a Running phase whose agent died with the workspace is Failed, not Running"
        );
        assert!(brainstorm.pane_id().is_none());
        assert_eq!(
            run.review_phases[0].status,
            PhaseStatus::Failed,
            "a reviewer is no more alive than a phase agent"
        );
        assert!(
            run.retired_panes.is_empty(),
            "retired panes died with the workspace too; cleanup must not chase them"
        );
    }

    /// The regression this repair could quietly introduce, pinned.
    ///
    /// Before re-provisioning existed, `phase start` on an ARCHIVED run failed
    /// because archiving destroys the workspace and nothing recreated one. That
    /// accident was the only thing enforcing the human's decision to file the run
    /// away — and repairing the workspace would remove it, so that `drovr phase
    /// start <archived-run>` would launch a live agent while the UI still shows the
    /// run as archived. Repair does not get to overrule the human.
    #[test]
    fn an_archived_run_is_not_quietly_brought_back_to_life() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("archived-ws-test", "wAG");
        run.archived = true;
        run.save().unwrap();
        h.kill_workspace("wAG", ["wAG:root".to_string()]);

        let err = phase_start(&h, &mut run, "implement", None)
            .expect_err("an archived run must not be resurrected by a repair");
        // The shared constructor, byte for byte — drovr's two hard refusals
        // (`archived_run_error`, `missing_project_dir_error`) are each written
        // once, so the guidance cannot drift between the sites that raise them.
        assert_eq!(
            err.to_string(),
            archived_run_error("archived-ws-test").to_string()
        );
        let msg = err.to_string();
        assert!(msg.contains("archived"), "say why it refused: {msg}");
        assert!(
            msg.contains("Restore"),
            "and how to undo it, since the run itself is fine: {msg}"
        );
        assert!(
            !h.calls().iter().any(|c| c.contains("workspace_create")),
            "no workspace may be created for a run the human filed away: {:?}",
            h.calls()
        );
    }

    /// A failed repair hands back the run EXACTLY as it was given.
    ///
    /// The orphan-workspace problem one level in: a caller holding a `RunState`
    /// that has been half-repaired — new workspace id, cleared pane ids, phases
    /// demoted — holds something that looks like a repaired run and is not one,
    /// and the next `save` anywhere writes that fiction to disk. Nothing about the
    /// type stops it, and the caller who would get it wrong is precisely the one
    /// not reading a "do not save this" comment. So the mutation happens on a copy
    /// that is committed only once it is on disk, and this pins it: every failure
    /// mode, compared field by field through serde.
    #[test]
    fn a_failed_repair_leaves_the_callers_run_state_untouched() {
        let _lock = ENV_LOCK.lock().unwrap();

        // 1. Refused because the run is archived.
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("untouched-archived-test", "wAG");
        run.phases.push({
                let mut p = Phase::new("plan");
                p.status = PhaseStatus::Running;
                p
            }
            .with_pane("wAG:p1"));
        run.archived = true;
        run.save().unwrap();
        h.kill_workspace("wAG", ["wAG:root".to_string()]);
        let before = serde_json::to_string(&run).unwrap();
        ensure_workspace(&h, &mut run).expect_err("archived");
        assert_eq!(
            serde_json::to_string(&run).unwrap(),
            before,
            "a refusal must not leave the caller holding a changed run"
        );

        // 2. Refused because there is no cwd to open a workspace in.
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("untouched-nocwd-test", "wAG");
        run.project_dir = String::new();
        run.save().unwrap();
        h.kill_workspace("wAG", ["wAG:root".to_string()]);
        let before = serde_json::to_string(&run).unwrap();
        ensure_workspace(&h, &mut run).expect_err("no project_dir");
        assert_eq!(serde_json::to_string(&run).unwrap(), before);

        // 3. herdr refused to make the workspace.
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("untouched-create-test", "wAG");
        run.phases.push({
                let mut p = Phase::new("plan");
                p.status = PhaseStatus::Running;
                p
            }
            .with_pane("wAG:p1"));
        run.retire_pane("wAG:p7");
        run.save().unwrap();
        h.kill_workspace("wAG", ["wAG:root".to_string()]);
        h.fail_workspace_create();
        let before = serde_json::to_string(&run).unwrap();
        ensure_workspace(&h, &mut run).expect_err("workspace_create fails");
        assert_eq!(
            serde_json::to_string(&run).unwrap(),
            before,
            "the demotions and pane clearing must not survive a failed create"
        );

        // 4. THE case the other three do not reach: the workspace was created and
        //    the run mutated, and only the SAVE failed. Cases 1-3 return before any
        //    mutation, so they would pass even against a version that leaves a
        //    half-repaired run behind — this is the one that pins it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let h = FakeHerdr::new();
            let mut run = make_run_with_workspace("untouched-save-test", "wAG");
            run.phases.push({
                    let mut p = Phase::new("plan");
                    p.status = PhaseStatus::Running;
                    p
                }
                .with_pane("wAG:p1"));
            run.retire_pane("wAG:p7");
            run.save().unwrap();
            h.kill_workspace("wAG", ["wAG:root".to_string()]);

            let dir = run_dir("untouched-save-test");
            let original = std::fs::metadata(&dir).unwrap().permissions();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
            let writable_anyway = std::fs::write(dir.join(".probe"), b"x").is_ok();
            let _ = std::fs::remove_file(dir.join(".probe"));
            if !writable_anyway {
                let before = serde_json::to_string(&run).unwrap();
                let err = ensure_workspace(&h, &mut run).expect_err("the save cannot succeed");
                std::fs::set_permissions(&dir, original).unwrap();
                assert_eq!(
                    serde_json::to_string(&run).unwrap(),
                    before,
                    "a repair that could not be persisted must not leave the caller \
                     holding a run that looks repaired: {err}"
                );
            } else {
                std::fs::set_permissions(&dir, original).unwrap();
            }
        }
    }

    /// A workspace drovr creates but cannot RECORD is worse than one it never
    /// created: `state.json` still names the dead one, so the next attempt makes a
    /// second replacement, while the first sits in the human's switcher labelled
    /// `drovr:<run>` with nothing pointing at it. The repair is only complete once
    /// it is persisted, so a failed save gives the workspace back.
    ///
    /// Uses a read-only run directory to make the save fail — the failure this
    /// models is a transient ENOSPC/EACCES, and there is no other deterministic
    /// way to reach it. Skipped when the test user can write to a read-only
    /// directory anyway (root), rather than asserting something untrue.
    #[test]
    #[cfg(unix)]
    fn a_workspace_that_cannot_be_recorded_is_given_back() {
        use std::os::unix::fs::PermissionsExt;

        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("ws-save-fails-test", "wAG");
        run.save().unwrap();
        h.kill_workspace("wAG", ["wAG:root".to_string()]);

        let dir = run_dir("ws-save-fails-test");
        let original = std::fs::metadata(&dir).unwrap().permissions();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        // Root ignores the mode bits, so the premise would be false there.
        let writable_anyway = std::fs::write(dir.join(".probe"), b"x").is_ok();
        let _ = std::fs::remove_file(dir.join(".probe"));
        if writable_anyway {
            std::fs::set_permissions(&dir, original).unwrap();
            return;
        }

        let err = ensure_workspace(&h, &mut run).expect_err("the save cannot succeed");
        std::fs::set_permissions(&dir, original).unwrap();

        let calls = h.calls();
        let created: Vec<&String> = calls
            .iter()
            .filter(|c| c.contains("workspace_create"))
            .collect();
        assert_eq!(created.len(), 1, "one workspace was created: {calls:?}");
        // Whatever id the fake handed out, the SAME one must be closed again.
        // Read it out of the create call, NOT out of `run`: a failed repair leaves
        // the caller's run untouched (`a_failed_repair_leaves_the_callers_run_state_untouched`),
        // so `run.workspace` still names the dead one — which is the point.
        let new_id = created[0]
            .rsplit(" -> ")
            .next()
            .and_then(|tail| tail.split_whitespace().next())
            .expect("the create call records the id it handed out")
            .to_string();
        assert_ne!(new_id, "wAG", "the fake handed out a fresh id");
        assert!(
            calls
                .iter()
                .any(|c| c == &format!("workspace_close id={new_id}")),
            "the workspace it could not record must be handed back: {calls:?}"
        );
        assert_eq!(
            run.workspace.as_deref(),
            Some("wAG"),
            "and the caller is left pointing at the dead workspace, not at one that \
             no longer exists"
        );
        // And the error has to name the save failure, not the reclaim.
        assert!(
            err.to_string().contains("could not record"),
            "the error must say the repair did not stick: {err}"
        );
        assert_eq!(
            RunState::load("ws-save-fails-test")
                .unwrap()
                .workspace
                .as_deref(),
            Some("wAG"),
            "nothing was persisted, so the next attempt starts from the same place"
        );
    }

    /// Consulting `archived` means refreshing it, and refreshing means the copy in
    /// hand now AGREES with disk. Reading disk for the guard while leaving the
    /// stale flag in place is how a repair ends up re-archiving, on its success
    /// path, the very run it just repaired: `save_preserving_archived` writes at
    /// the end of `ensure_workspace`, and it writes what the copy holds.
    #[test]
    fn repairing_a_restored_run_leaves_it_restored_on_disk() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("archived-writeback-test", "wAG");
        run.archived = false;
        run.save().unwrap();
        // This caller's copy is latched `true` from an Archive it observed before
        // the human changed their mind — exactly what `save_preserving_archived`
        // used to leave behind.
        run.archived = true;
        h.kill_workspace("wAG", ["wAG:root".to_string()]);

        phase_start(&h, &mut run, "implement", None).expect("a restored run is repairable");

        assert!(
            !RunState::load("archived-writeback-test").unwrap().archived,
            "the repair must not write a stale `archived: true` back over a Restore"
        );
        assert!(
            !run.archived,
            "and the copy in hand must agree with what it consulted"
        );
    }

    /// The guard is load-bearing, so it fails CLOSED. An unreadable `state.json`
    /// is not evidence that the run is un-archived, and quietly falling back to
    /// the caller's copy would let a torn read re-provision a run the human filed
    /// away — the guard skipped by an error nobody sees.
    #[test]
    fn an_unreadable_state_json_refuses_the_repair_rather_than_guessing() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("archived-unreadable-test", "wAG");
        run.save().unwrap();
        std::fs::write(
            run_dir("archived-unreadable-test").join("state.json"),
            b"{ torn",
        )
        .unwrap();
        h.kill_workspace("wAG", ["wAG:root".to_string()]);

        let err = phase_start(&h, &mut run, "implement", None)
            .expect_err("an unreadable state.json must not be read as 'not archived'");
        assert!(
            !h.calls().iter().any(|c| c.contains("workspace_create")),
            "and nothing may be created on that non-answer: {:?}",
            h.calls()
        );
        // The message has to point at the file, since that is what must be fixed.
        assert!(err.to_string().contains("state.json"), "{err}");
    }

    /// The archive flag a caller holds in memory can be STALE IN BOTH DIRECTIONS,
    /// and only one of them is the human's current decision.
    ///
    /// A caller acquires a stale `true` simply by observing an Archive: a
    /// `code-review run` whose panel is archived mid-flight loads it from disk on
    /// its next save (`archiving_mid_run_survives_every_save_the_review_makes`)
    /// and then holds it for as long as it holds that `RunState`. If the human
    /// then hits Restore, that copy must not be what decides whether the run may
    /// be repaired. Disk is where Archive and Restore both land, so disk wins —
    /// see `RunState::archived`, which states that rule for every site.
    ///
    /// (Until `4865d1d` `save_preserving_archived` also *merged* with `|=`, so
    /// such a copy could additionally write its stale `true` back over the
    /// Restore. That is fixed — it adopts disk's value now — but this test pins
    /// the read side, which would still be wrong on its own.)
    #[test]
    fn a_restore_on_disk_beats_a_stale_archive_held_in_memory() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("archived-stale-test", "wAG");
        // The human's current decision, as recorded by the Restore button.
        run.archived = false;
        run.save().unwrap();
        // ...and this caller's copy, latched `true` by an Archive it saw earlier.
        run.archived = true;
        h.kill_workspace("wAG", ["wAG:root".to_string()]);

        phase_start(&h, &mut run, "implement", None)
            .expect("a restored run must be repairable despite a caller's stale flag");
        assert!(
            h.calls().iter().any(|c| c.contains("workspace_create")),
            "the workspace must be rebuilt: {:?}",
            h.calls()
        );
    }

    /// The other side of that guard: an archived run whose `workspace_close`
    /// FAILED still has live panes (drovr's "zombie"), and `phase_start` on one
    /// has always been able to reuse them. The guard must not change that — it
    /// exists to stop repair overruling the human, not to add a new refusal.
    #[test]
    fn an_archived_run_whose_workspace_survived_still_starts() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("archived-zombie-test", "wAG");
        run.archived = true;
        run.save().unwrap();
        // workspace_close failed, so wAG is still there.

        phase_start(&h, &mut run, "implement", None)
            .expect("a zombie's live panes are still usable, as before");
        assert!(
            !h.calls().iter().any(|c| c.contains("workspace_create")),
            "nothing needed creating: {:?}",
            h.calls()
        );
    }

    /// The other half of the same refusal: `drovr new` warns and records no
    /// workspace when creation fails, and `phase start` used to answer that with
    /// "please recreate the run" — for a run that may hold 23 tasks of work.
    #[test]
    fn phase_start_provisions_a_workspace_the_run_never_got() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("ws-null-test");
        run.workspace = None;
        run.root_pane = None;

        phase_start(&h, &mut run, "implement", None)
            .expect("a run whose workspace creation failed must still be startable");

        assert!(run.workspace.is_some(), "a workspace must have been created");
        let calls = h.calls();
        assert!(
            calls.iter().any(|c| c.contains("workspace_create")),
            "must create the missing workspace rather than refuse: {calls:?}"
        );
    }

    /// The guard on all of the above: re-provisioning over a LIVE workspace would
    /// orphan the run's own agents, which is worse than the bug being fixed.
    #[test]
    fn a_live_workspace_is_never_reprovisioned() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("ws-live-test", "wAG");

        phase_start(&h, &mut run, "implement", None).unwrap();

        let calls = h.calls();
        assert!(
            !calls.iter().any(|c| c.contains("workspace_create")),
            "a live workspace must be left exactly as it is: {calls:?}"
        );
        assert_eq!(run.workspace.as_deref(), Some("wAG"));
    }

    /// Reviewers need their own tab, so they need a workspace just as much —
    /// `code-review run` spawns several in a loop and must not be the one command
    /// that still dies on a vanished one.
    #[test]
    fn spawn_reviewer_reprovisions_a_vanished_workspace() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("rev-ws-gone-test", "wAG");
        h.kill_workspace("wAG", ["wAG:root".to_string()]);

        spawn_reviewer(
            &h,
            &mut run,
            "review:t:1:correctness",
            None,
            &AgentLaunch::for_test("claude", "claude --permission-mode plan"),
        )
        .expect("a reviewer must survive a vanished workspace");

        let ws = run.workspace.clone().unwrap();
        assert_ne!(ws, "wAG");
        let calls = h.calls();
        assert!(
            calls
                .iter()
                .any(|c| c.contains(&format!("tab_create workspace={ws}"))),
            "the reviewer tab must be created in the new workspace: {calls:?}"
        );
    }

    // -- RED: write failing test first, then implement -----------------------

    #[test]
    fn start_records_pane_id_and_status_running() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("start-test");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();

        // Phase should be appended and marked Running
        assert_eq!(run.phases.len(), 1);
        let p = &run.phases[0];
        assert_eq!(p.name, "brainstorm");
        assert_eq!(p.status, PhaseStatus::Running);
        assert!(p.pane_id().is_some(), "pane_id must be recorded");
        // herdr_session is no longer written (cleanup closes panes by id, not session_stop)
        assert!(p.herdr_session.is_none(), "herdr_session must be None");
        // claude is launched via `pane run`, NOT a split-creating `agent start`.
        let calls = h.calls();
        assert!(
            !calls.iter().any(|c| c.contains("agent_start")),
            "must not use agent_start (it splits a new pane): {calls:?}"
        );
        let run_call = calls.iter().find(|c| c.contains("pane_run")).unwrap();
        assert!(
            run_call.contains("claude"),
            "pane_run must launch claude: {run_call}"
        );
        // The workspace-root guard pins edits to the project dir (issue 3): a
        // worktree run must not stray into the outer checkout.
        assert!(
            run_call.contains("--add-dir '/tmp/drovr-proj-test'"),
            "pane_run must add-dir the project root: {run_call}"
        );
        assert!(
            run_call.contains("--append-system-prompt"),
            "pane_run must append the workspace-root system prompt: {run_call}"
        );
    }

    #[test]
    fn reviewer_launch_includes_workspace_root_guard() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("rev-guard-test", "ws-g");

        spawn_reviewer(
            &h,
            &mut run,
            "review:t:1:correctness",
            None,
            &AgentLaunch::for_test(
                "claude",
                "claude --permission-mode plan --add-dir '/tmp/drovr-proj-test'",
            ),
        )
        .unwrap();

        let calls = h.calls();
        let run_call = calls.iter().find(|c| c.contains("pane_run")).unwrap();
        assert!(
            run_call.contains("claude --permission-mode plan"),
            "reviewer must keep its read-only launch flags: {run_call}"
        );
        assert!(
            run_call.contains("--add-dir '/tmp/drovr-proj-test'"),
            "reviewer must also get the workspace-root guard: {run_call}"
        );
    }

    /// The FIRST phase gets a tab of its own, exactly like every later one.
    ///
    /// It used to consume `run.root_pane` instead, which put a phase agent in
    /// the pane that anchors the whole workspace — so reaping that phase's tab
    /// would take the run's workspace with it. The root shell now stays idle
    /// for the run's lifetime and every phase tab is independently closeable.
    #[test]
    fn first_phase_creates_its_own_tab_and_leaves_the_root_pane_alone() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("ws-isolation-test", "ws-42");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();

        let calls = h.calls();
        let tab_call = calls
            .iter()
            .find(|c| c.contains("tab_create"))
            .expect("the first phase must create its own tab");
        assert!(
            tab_call.contains("workspace=ws-42"),
            "tab must be in the run workspace: {tab_call}"
        );
        assert!(
            tab_call.contains("label=brainstorm"),
            "tab must be labelled with the phase: {tab_call}"
        );
        let pane = run.phases[0].pane_id().map(str::to_owned).unwrap();
        assert_ne!(
            pane, "ws-42:root",
            "no phase may run in the workspace's root shell"
        );
        assert!(
            calls
                .iter()
                .any(|c| c.contains(&format!("pane_run pane={pane}"))),
            "claude must run in the new tab's pane: {calls:?}"
        );
        assert_eq!(
            run.root_pane.as_deref(),
            Some("ws-42:root"),
            "the root shell anchors the workspace for the whole run and is never claimed"
        );
    }

    #[test]
    fn later_phase_creates_its_own_tab() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("later-tab-test", "ws-7");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();
        phase_start(&h, &mut run, "plan", None).unwrap();

        let calls = h.calls();
        // Every phase creates a tab now, so select this phase's by its label —
        // a bare `find("tab_create")` would match brainstorm's.
        let tab_call = calls
            .iter()
            .find(|c| c.contains("tab_create") && c.contains("label=plan"))
            .expect("plan must create its own tab");
        assert!(
            tab_call.contains("workspace=ws-7"),
            "tab must be in the run workspace: {tab_call}"
        );
        // claude runs in the new tab's pane, and the two phases are in
        // different panes — neither shares, and neither is the root shell.
        let brainstorm_pane = run.phases[0].pane_id().map(str::to_owned).unwrap();
        let plan_pane = run.phases[1].pane_id().map(str::to_owned).unwrap();
        assert_ne!(brainstorm_pane, plan_pane, "phases must not share a pane");
        assert!(
            calls
                .iter()
                .any(|c| c.contains(&format!("pane_run pane={plan_pane}"))),
            "claude must run in the new tab's pane: {calls:?}"
        );
    }

    /// A workspace is now the ONLY way to place a phase: without one there is
    /// no tab to create, and the root pane is not a fallback even when present.
    #[test]
    fn a_workspace_that_cannot_be_opened_anywhere_says_which_state_is_missing() {
        // The successor to "phase_start must error when there is no workspace": a
        // missing workspace is now repaired (see
        // `phase_start_provisions_a_workspace_the_run_never_got`), so the only
        // remaining hard failure is the one piece of state that genuinely cannot
        // be rebuilt — a cwd to open the workspace in. Even then the run is not
        // sent back to `drovr new`: it names the missing field and where to put
        // it, because everything else about the run is still good.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("no-cwd-test");
        run.workspace = None;
        run.root_pane = None;
        run.project_dir = String::new();

        let err = ensure_workspace(&h, &mut run)
            .expect_err("no project_dir means no cwd for a workspace");
        let msg = err.to_string();
        assert!(
            msg.contains("project_dir"),
            "must name what is missing: {msg}"
        );
        assert!(
            !msg.contains("recreate the run"),
            "a lost workspace must never be answered with 'start over': {msg}"
        );
        assert!(
            !h.calls().iter().any(|c| c.contains("workspace_create")),
            "must not create a workspace with no cwd — that is how a pane opens in \
             an unrelated repo: {:?}",
            h.calls()
        );
    }

    #[test]
    fn phase_start_preserves_focus() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("focus-test");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();

        let calls = h.calls();
        let capture = calls.iter().position(|c| c.contains("focused_workspace"));
        let run_at = calls.iter().position(|c| c.contains("pane_run"));
        let restore = calls.iter().position(|c| c.contains("workspace_focus"));
        let (capture, run_at, restore) = (capture.unwrap(), run_at.unwrap(), restore.unwrap());
        assert!(
            capture < run_at,
            "focus must be captured before pane_run: {calls:?}"
        );
        assert!(
            restore > run_at,
            "focus must be restored after pane_run: {calls:?}"
        );
        assert!(
            calls[restore].contains("workspace_focus id=ws-focused"),
            "focus must be restored to the previously-focused workspace: {}",
            calls[restore]
        );
    }

    #[test]
    fn wait_sees_marker_and_marks_phase_done() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("wait-done-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        // The phase agent authors its handoff, then signals completion — from
        // inside its pane, so the marker carries this pass's token.
        write_handoff(&run, "plan");
        let pass = run.phases[0].pass.clone().unwrap();
        agent_signals_done(&run, "plan", &pass);
        let marker = done_marker(&run.name, "plan");
        assert!(
            marker.exists(),
            "marker should exist at {}",
            marker.display()
        );

        let outcome = phase_wait(&h, &mut run, "plan", 5000).unwrap();
        assert_eq!(outcome, PhaseWaitOutcome::Done);
        assert_eq!(run.phases[0].status, PhaseStatus::Done);
    }

    // -- Neither `phase_start` nor `phase_wait` may un-archive a run ----------
    //
    // Both hold a `RunState` loaded before they block, and the human can archive
    // from the web UI in between — which closes the run's herdr workspace. A
    // plain `save` writes the stale `archived: false` back, so a run the human
    // filed away comes back looking active while every one of its panes is gone.
    // (Since 2026-08-02 the workspace itself is recoverable — `ensure_workspace`
    // rebuilds one — but only after a Restore, which is exactly the decision this
    // flag records.) These drive the real call sites: mutating either back to
    // `save()` fails here.

    /// Model the reviewer archiving `run` from the web UI: a separate load,
    /// flag, save — exactly what the archive endpoint does — while `run`'s
    /// in-memory copy still says `archived: false`.
    fn archive_on_disk(name: &str) {
        let mut disk = RunState::load(name).expect("run is on disk");
        disk.archived = true;
        disk.save().expect("archive it");
    }

    #[test]
    fn phase_start_does_not_un_archive_a_run_archived_while_it_worked() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let h = FakeHerdr::new();
        let mut run = make_run("phase-start-archived");
        run.save().unwrap();
        archive_on_disk("phase-start-archived");

        phase_start(&h, &mut run, "plan", None).unwrap();

        assert!(
            RunState::load("phase-start-archived").unwrap().archived,
            "phase_start's save must not resurrect a run archived while it ran"
        );
    }

    #[test]
    fn phase_send_does_not_un_archive_a_run_archived_while_it_reopened() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let h = FakeHerdr::new();
        let mut run = make_run("phase-send-archived");

        phase_start(&h, &mut run, "plan", None).unwrap();
        let pass = run.phases[0].pass.clone().unwrap();
        write_handoff(&run, "plan");
        agent_signals_done(&run, "plan", &pass);
        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::Done
        );

        // The reviewer archives while the driver is between the wait and the
        // re-entry send. `reopen_for_re_entry`'s save is the fourth of the
        // snapshot writers converted to preserving, and the only one that had no
        // test of its own — the existing archive-mid-review tests fire during
        // `spawn_reviewer` or the poll loop, never during a plain `phase send`.
        archive_on_disk("phase-send-archived");

        phase_send(&h, &mut run, "plan", "carry on").unwrap();

        assert!(
            RunState::load("phase-send-archived").unwrap().archived,
            "phase_send's re-entry save must not resurrect a run archived while it \
             was re-opening the phase"
        );
    }

    #[test]
    fn phase_wait_does_not_un_archive_a_run_archived_while_it_blocked() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let h = FakeHerdr::new();
        let mut run = make_run("phase-wait-archived");

        phase_start(&h, &mut run, "plan", None).unwrap();
        // The phase agent authors its handoff and signals completion from inside
        // its pane, so the marker carries THIS pass's token — an untokenized one
        // is now ignored, which would time out instead of completing.
        write_handoff(&run, "plan");
        let pass = run.phases[0].pass.clone().unwrap();
        agent_signals_done(&run, "plan", &pass);
        // ...but the reviewer archived the run while the wait was blocked.
        archive_on_disk("phase-wait-archived");

        let outcome = phase_wait(&h, &mut run, "plan", 2000).unwrap();

        assert_eq!(outcome, PhaseWaitOutcome::Done);
        assert!(
            RunState::load("phase-wait-archived").unwrap().archived,
            "phase_wait's save must not resurrect a run archived while it blocked"
        );
    }

    #[test]
    fn wait_timeout_leaves_running() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("wait-timeout-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        // No marker dropped and no blocked status → wait times out quickly and
        // leaves the phase Running.
        let outcome = phase_wait(&h, &mut run, "plan", 50).unwrap();
        assert_eq!(outcome, PhaseWaitOutcome::TimedOut);
        assert_eq!(run.phases[0].status, PhaseStatus::Running);
    }

    // -- Task 1: phase_wait detects a blocked pane early ----------------------

    #[test]
    fn wait_blocked_short_circuits_before_timeout() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("wait-blocked-fast-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        h.push_status(Some("blocked"));

        let start = Instant::now();
        let outcome = phase_wait(&h, &mut run, "plan", 60_000).unwrap();
        assert_eq!(outcome, PhaseWaitOutcome::Blocked);
        // Must have short-circuited on the first poll, not waited out the timeout.
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "blocked must return promptly"
        );
        // Blocked leaves the phase Running (not Done) — the driver escalates.
        assert_eq!(run.phases[0].status, PhaseStatus::Running);
    }

    #[test]
    fn wait_idle_status_keeps_waiting_not_blocked_or_done() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("wait-idle-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        // idle is NOT a completion or block signal (a parked agent awaiting a
        // subagent is idle) — it must keep waiting and then time out.
        h.push_status(Some("idle"));
        h.push_status(Some("working"));

        let outcome = phase_wait(&h, &mut run, "plan", 50).unwrap();
        assert_eq!(outcome, PhaseWaitOutcome::TimedOut);
        assert_eq!(run.phases[0].status, PhaseStatus::Running);
    }

    #[test]
    fn wait_marker_wins_over_status_check() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("wait-marker-wins-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        // Marker present AND a blocked status queued: the marker WINS, so the
        // phase is Done rather than Blocked.
        write_handoff(&run, "plan");
        let pass = run.phases[0].pass.clone().unwrap();
        agent_signals_done(&run, "plan", &pass);
        h.push_status(Some("blocked"));

        let outcome = phase_wait(&h, &mut run, "plan", 5000).unwrap();
        assert_eq!(outcome, PhaseWaitOutcome::Done);
        assert_eq!(run.phases[0].status, PhaseStatus::Done);

        // The pane IS polled, and that is deliberate. This test used to assert
        // the opposite — that a present marker meant the pane was never read —
        // and that assertion was pinning a bug: the poll is where the session is
        // captured, and a phase whose marker is already on disk at the first
        // iteration would never have been polled at all. Precedence is what
        // matters here (marker beats status), not whether the pane was touched.
        assert!(
            h.calls().iter().any(|c| c.contains("agent_status")),
            "the pane must still be polled — that is where capture happens: {:?}",
            h.calls()
        );
    }

    // -- Task 2: classify_blocked_prompt (pure) -------------------------------

    #[test]
    fn classify_destructive_rm() {
        assert_eq!(
            classify_blocked_prompt("Dangerous rm operation detected. 1. Yes 2. No"),
            BlockedClass::Destructive
        );
        assert_eq!(
            classify_blocked_prompt("run `git reset --hard origin/main`? 1. Yes"),
            BlockedClass::Destructive
        );
        assert_eq!(
            classify_blocked_prompt("force push to main? 1. Yes 2. No"),
            BlockedClass::Destructive
        );
    }

    #[test]
    fn classify_routine_edit_permission() {
        assert_eq!(
            classify_blocked_prompt("Do you want to make this edit to src/main.rs? 1. Yes"),
            BlockedClass::Routine
        );
    }

    #[test]
    fn classify_unknown_escalates() {
        // A permission-style prompt not on the allow-list is Unknown, not Routine.
        assert_eq!(
            classify_blocked_prompt("Allow the WebFetch tool to run? 1. Yes 2. No"),
            BlockedClass::Unknown
        );
        assert_eq!(
            classify_blocked_prompt("ordinary working output"),
            BlockedClass::Unknown
        );
    }

    #[test]
    fn classify_destructive_wins_over_routine() {
        // A prompt that reads like an edit permission but whose target is a
        // destructive command must escalate, never auto-answer.
        let pane = "Do you want to make this edit? It will delete the whole directory. 1. Yes";
        assert_eq!(classify_blocked_prompt(pane), BlockedClass::Destructive);
    }

    #[test]
    fn classify_is_case_insensitive() {
        assert_eq!(
            classify_blocked_prompt("DANGEROUS RM"),
            BlockedClass::Destructive
        );
        assert_eq!(
            classify_blocked_prompt("DO YOU WANT TO MAKE THIS EDIT"),
            BlockedClass::Routine
        );
    }

    // -- Task 2: triage_blocked_phase -----------------------------------------

    #[test]
    fn triage_destructive_escalates_and_never_auto_answers() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("triage-destructive-test");

        phase_start(&h, &mut run, "code", None).unwrap();
        h.push_read("Dangerous rm operation detected.\n❯ 1. Yes\n  2. No");

        let t = triage_blocked_phase(&h, &run, "code");
        assert_eq!(t.class, BlockedClass::Destructive);
        assert!(
            !t.auto_answered,
            "destructive prompts must never be auto-answered"
        );
        assert!(
            t.diagnostic.contains("triage-destructive-test"),
            "names run: {}",
            t.diagnostic
        );
        assert!(
            t.diagnostic.contains("code"),
            "names phase: {}",
            t.diagnostic
        );
        assert!(
            t.diagnostic.contains("Dangerous rm"),
            "quotes prompt: {}",
            t.diagnostic
        );
        assert!(
            t.diagnostic
                .contains("drovr attach 'triage-destructive-test'"),
            "suggests attach: {}",
            t.diagnostic
        );
        // Crucially, NO keystroke was sent to the pane.
        assert!(
            !h.calls().iter().any(|c| c.contains("agent_send")),
            "must not send any keystroke on a destructive prompt: {:?}",
            h.calls()
        );
    }

    #[test]
    fn triage_routine_auto_answers_and_continues() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("triage-routine-test");

        phase_start(&h, &mut run, "code", None).unwrap();
        h.push_read("Do you want to make this edit to src/lib.rs?\n❯ 1. Yes\n  2. No");

        let t = triage_blocked_phase(&h, &run, "code");
        assert_eq!(t.class, BlockedClass::Routine);
        assert!(
            t.auto_answered,
            "routine allow-listed prompt should be auto-answered"
        );
        assert!(
            t.diagnostic.contains("auto-answered"),
            "diagnostic notes auto-answer: {}",
            t.diagnostic
        );
        // The accept keystroke was sent to the phase pane.
        let pane_id = run.phases[0].pane_id().map(str::to_owned).unwrap();
        assert!(
            h.calls()
                .iter()
                .any(|c| c.contains("agent_send") && c.contains(&pane_id)),
            "must send accept keystroke to the pane: {:?}",
            h.calls()
        );
    }

    #[test]
    fn triage_unknown_escalates_without_answering() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("triage-unknown-test");

        phase_start(&h, &mut run, "code", None).unwrap();
        h.push_read("Allow the WebFetch tool to run?\n❯ 1. Yes\n  2. No");

        let t = triage_blocked_phase(&h, &run, "code");
        assert_eq!(t.class, BlockedClass::Unknown);
        assert!(
            !t.auto_answered,
            "unknown prompts must escalate, not auto-answer"
        );
        assert!(
            t.diagnostic.contains("drovr attach"),
            "suggests attach: {}",
            t.diagnostic
        );
        assert!(
            !h.calls().iter().any(|c| c.contains("agent_send")),
            "must not answer an unknown prompt: {:?}",
            h.calls()
        );
    }

    #[test]
    fn triage_no_pane_id_escalates() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("triage-no-pane-test");
        // A phase with no pane_id yet.
        run.phases.push({
            let mut p = Phase::new("code");
            p.status = PhaseStatus::Running;
            p
        });

        let t = triage_blocked_phase(&h, &run, "code");
        assert_eq!(t.class, BlockedClass::Unknown);
        assert!(!t.auto_answered);
        assert!(
            !h.calls().iter().any(|c| c.contains("agent_read")),
            "must not read a nonexistent pane: {:?}",
            h.calls()
        );
    }

    #[test]
    fn done_on_unknown_phase_errors() {
        let _lock = ENV_LOCK.lock().unwrap();
        let mut run = make_run("done-unknown-test");
        // No phases registered → phase_done must reject rather than write a
        // stray marker.
        assert!(phase_done(&run, "nonexistent").is_err());
        // Even with a review phase present, a name in NEITHER list is rejected.
        run.review_phases.push({
            let mut p = Phase::new("review:t:1:correctness");
            p.status = PhaseStatus::Running;
            p
        });
        assert!(phase_done(&run, "nonexistent").is_err());
    }

    #[test]
    fn done_succeeds_for_review_phase() {
        let _lock = ENV_LOCK.lock().unwrap();
        let mut run = make_run("done-review-test");
        // A reviewer phase lives only in `review_phases`, yet must be able to drop
        // its completion marker via `drovr phase done` (which calls phase_done).
        run.review_phases.push(
            {
                let mut p = Phase::new("review:t:1:correctness");
                p.status = PhaseStatus::Running;
                p
            }
            .with_pane("rp1"),
        );
        let marker = phase_done(&run, "review:t:1:correctness").unwrap();
        assert!(
            marker.exists(),
            "marker should exist at {}",
            marker.display()
        );
    }

    // -- self-authored handoff: `phase done` enforces the handoff exists ----------

    #[test]
    fn done_requires_handoff_for_pipeline_phase() {
        let _lock = ENV_LOCK.lock().unwrap();
        let mut run = make_run("done-requires-handoff");
        run.phases.push(
            {
                let mut p = Phase::new("plan");
                p.status = PhaseStatus::Running;
                p
            }
            .with_pane("p1"),
        );
        // The finishing agent authors <phase>-HANDOFF.md itself, in-context, BEFORE
        // signalling done. With no handoff present, phase_done must refuse — the
        // completion contract is atomic (no marker without a handoff).
        let res = phase_done(&run, "plan");
        assert!(
            res.is_err(),
            "phase done must refuse a pipeline phase with no handoff"
        );
        assert!(
            res.unwrap_err()
                .to_string()
                .to_lowercase()
                .contains("handoff"),
            "error must name the missing handoff"
        );
        // The marker must NOT have been written on the refused call.
        assert!(
            !done_marker(&run.name, "plan").exists(),
            "no marker may be written when the handoff is missing"
        );
        // Author a non-empty handoff → done succeeds and drops the marker.
        let hp = run_dir(&run.name).join("plan-HANDOFF.md");
        std::fs::create_dir_all(hp.parent().unwrap()).unwrap();
        std::fs::write(&hp, "## Objective\nreal handoff\n").unwrap();
        let marker = phase_done(&run, "plan").unwrap();
        assert!(marker.exists());
    }

    #[test]
    fn done_rejects_empty_handoff_for_pipeline_phase() {
        let _lock = ENV_LOCK.lock().unwrap();
        let mut run = make_run("done-empty-handoff");
        run.phases.push(
            {
                let mut p = Phase::new("plan");
                p.status = PhaseStatus::Running;
                p
            }
            .with_pane("p1"),
        );
        // A whitespace-only handoff is treated as absent (guards the degenerate
        // 2-line-garbage case the old compressor produced).
        let hp = run_dir(&run.name).join("plan-HANDOFF.md");
        std::fs::create_dir_all(hp.parent().unwrap()).unwrap();
        std::fs::write(&hp, "   \n\n").unwrap();
        assert!(
            phase_done(&run, "plan").is_err(),
            "an empty/whitespace handoff must be rejected"
        );
        assert!(
            !done_marker(&run.name, "plan").exists(),
            "no marker may be written when the handoff is empty"
        );
    }

    #[test]
    fn send_routes_to_review_phase_pane() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-review-test");
        // A reviewer pane registered only in `review_phases` must still be
        // reachable by `phase_send` (require_pane_id falls back to review_phases).
        run.review_phases.push(
            {
                let mut p = Phase::new("review:t:1:correctness");
                p.status = PhaseStatus::Running;
                p
            }
            .with_pane("review-pane-9"),
        );
        // Report the pane ready so the readiness gate returns on the first poll.
        h.push_status(Some("idle"));
        phase_send(&h, &mut run, "review:t:1:correctness", "seed text").unwrap();
        let calls = h.calls();
        let send_call = calls
            .iter()
            .find(|c| c.contains("agent_prompt_confirm"))
            .unwrap();
        assert!(
            send_call.contains("review-pane-9"),
            "must route to the reviewer pane: {send_call}"
        );
        assert!(send_call.contains("seed text"));
    }

    #[test]
    fn send_routes_to_pane() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-test");

        phase_start(&h, &mut run, "code", None).unwrap();
        // Report the pane ready so the readiness gate returns on the first poll.
        h.push_status(Some("idle"));
        phase_send(&h, &mut run, "code", "hello agent").unwrap();

        // The delivery-confirming prompt carries the text and the pane.
        let calls = h.calls();
        let send_call = calls
            .iter()
            .find(|c| c.contains("agent_prompt_confirm"))
            .unwrap();
        assert!(send_call.contains("hello agent"));
        // Target should match the pane_id recorded
        let pane_id = run.phases[0].pane_id().unwrap();
        assert!(send_call.contains(pane_id));
    }

    /// Confirm timeout for tests. `FakeHerdr` never actually waits, so this only
    /// has to be a distinguishable value — it never costs wall-clock.
    const CONFIRM: Duration = Duration::from_secs(15);

    // -- pane_shows_payload: the swallowed-vs-unsubmitted discriminator ---------

    // Captured verbatim from a real `cursor-agent` pane holding a briefing it had
    // typed but not submitted.
    const CURSOR_UNSUBMITTED_PANE: &str = "\
  v2026.06.24-00-45-58-9f61de7
  Tip: Try Cursor Grok 4.5 via /model.


  → [Pasted text #1 +5 lines]


  Composer 2.5                                        Run Everything
  /tmp/scratchpad/e2e/proj";

    // Captured verbatim from a real `claude` pane parked on the MCP approval menu
    // that had just SWALLOWED a briefing whole.
    const CLAUDE_MCP_MODAL_PANE: &str = "\
  New MCP server found in this project: probe2

  MCP servers may execute code or access system resources. All tool calls require approval.

  ❯ 1. Use this MCP server
    2. Use this and all future MCP servers in this project
    3. Continue without using this MCP server

  Enter to confirm · Esc to cancel";

    #[test]
    fn pane_shows_payload_accepts_a_collapsed_paste() {
        assert!(pane_shows_payload(
            CURSOR_UNSUBMITTED_PANE,
            "# Briefing Alpha Marker\n\nlots of context"
        ));
    }

    #[test]
    fn pane_shows_payload_accepts_a_short_prompt_echoed_verbatim() {
        let pane = "  → Reply with exactly: PONG\n\n  Composer 2.5";
        assert!(pane_shows_payload(pane, "Reply with exactly: PONG"));
    }

    // The regression that matters most: a modal that swallowed the payload must
    // NOT read as delivered, or `phase_send` presses Enter and accepts it. The
    // pane here differs wildly from anything sent — which is exactly why a
    // before/after DIFF cannot be the test.
    #[test]
    fn pane_shows_payload_rejects_a_modal_that_swallowed_the_seed() {
        assert!(!pane_shows_payload(
            CLAUDE_MCP_MODAL_PANE,
            "# Briefing Alpha Marker\n\nlots of context"
        ));
    }

    // A payload echoed by an EARLIER send, now scrolled up out of the composer,
    // is not evidence that this one landed.
    #[test]
    fn pane_shows_payload_ignores_evidence_above_the_composer_region() {
        let pane = format!(
            "  ❯ [Pasted text #1 +99 lines]\n{}\n{}",
            (0..COMPOSER_TAIL_LINES)
                .map(|i| format!("  transcript line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
            "  Enter to confirm · Esc to cancel"
        );
        assert!(!pane_shows_payload(&pane, "# Briefing\n\nbody"));
    }

    // A first line too short to be distinctive must not count as evidence — it
    // would match ordinary pane chrome by accident.
    #[test]
    fn pane_shows_payload_rejects_a_too_generic_fragment() {
        let pane = "  Do you trust this?\n  ❯ [a] Trust\n  # Go";
        assert!(!pane_shows_payload(pane, "# Go\n\nrest of the briefing"));
    }

    // -- delivery confirmation: the two ways a "successful" send loses the seed --

    // The healthy path (a claude send that self-submits): the prompt takes on the
    // first try, so no keystroke is sent and no second wait is needed.
    #[test]
    fn send_does_not_nudge_when_prompt_takes_first_try() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-healthy-test");

        phase_start(&h, &mut run, "code", None).unwrap();
        h.push_status(Some("idle"));
        h.push_outcome(PromptOutcome::Started);

        phase_send_with_timeout(
            &h,
            &mut run,
            "code",
            "hello agent",
            Duration::from_secs(5),
            Duration::from_millis(1),
            CONFIRM,
        )
        .unwrap();

        let calls = h.calls();
        assert!(
            !calls.iter().any(|c| c.contains("agent_send_keys")),
            "a delivered prompt must not be nudged: {calls:?}"
        );
        assert!(
            !calls.iter().any(|c| c.contains("agent_wait_started")),
            "no second wait is needed once the prompt took: {calls:?}"
        );
    }

    // The failure that motivated all of this: `agent.prompt` types the payload but
    // it is never submitted, so it sits in the composer and the agent never
    // starts. The payload is VISIBLY there, which is what licenses the Enter nudge
    // — and once nudged, the send is a success, not an error.
    #[test]
    fn send_nudges_enter_when_payload_lands_unsubmitted() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-unsubmitted-test");

        phase_start(&h, &mut run, "code", None).unwrap();
        h.push_status(Some("idle"));
        // Pane before the prompt (no payload), then after the stall: the payload
        // is visibly sitting in the composer, so it landed — just unsubmitted.
        h.push_read("> Plan, search, build anything\n  Composer 2.5");
        h.push_read("> [Pasted text #1 +40 lines]\n  Composer 2.5");
        // The prompt stalls; the follow-up Enter gets it moving.
        h.push_outcome(PromptOutcome::Stalled);
        h.push_outcome(PromptOutcome::Started);

        phase_send_with_timeout(
            &h,
            &mut run,
            "code",
            "hello agent",
            Duration::from_secs(5),
            Duration::from_millis(1),
            CONFIRM,
        )
        .unwrap();

        let calls = h.calls();
        let pane_id = run.phases[0].pane_id().map(str::to_owned).unwrap();
        let keys = calls
            .iter()
            .find(|c| c.contains("agent_send_keys"))
            .unwrap_or_else(|| panic!("must nudge the composer with Enter: {calls:?}"));
        assert!(
            keys.contains("enter") && keys.contains(&pane_id),
            "the Enter must go to the phase pane: {keys}"
        );
        assert!(
            calls.iter().any(|c| c.contains("agent_wait_started")),
            "must confirm the nudge actually got the agent moving: {calls:?}"
        );
    }

    // THE SAFETY PROPERTY. The prompt was swallowed whole, so the pane is
    // unchanged. `phase_send` must RAISE and must NOT press a key: Enter on
    // whatever dialog is up accepts its highlighted option on the user's behalf.
    // Assert the ABSENCE of the keystroke, not merely the error.
    #[test]
    fn send_raises_without_keystroke_when_payload_is_swallowed() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-swallowed-test");

        phase_start(&h, &mut run, "code", None).unwrap();
        // A pane parked on an unclassified modal still reports `idle`, so the
        // readiness gate waves it through — this is reachable, not hypothetical.
        h.push_status(Some("idle"));
        // The modal ate the prompt: before and after, the composer region shows
        // the dialog and no trace of the payload.
        h.push_read(CLAUDE_MCP_MODAL_PANE);
        h.push_read(CLAUDE_MCP_MODAL_PANE);
        h.push_outcome(PromptOutcome::Stalled);

        let err = phase_send_with_timeout(
            &h,
            &mut run,
            "code",
            "hello agent",
            Duration::from_secs(5),
            Duration::from_millis(1),
            CONFIRM,
        )
        .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(
            err.to_string().contains("send-swallowed-test") && err.to_string().contains("code"),
            "error must name the run and phase: {err}"
        );
        assert!(
            err.to_string().contains("drovr attach"),
            "error must suggest attach: {err}"
        );
        assert!(
            !h.calls().iter().any(|c| c.contains("agent_send_keys")),
            "must NOT answer the dialog on the user's behalf: {:?}",
            h.calls()
        );
    }

    // Evidence must have APPEARED. A long-lived pane still showing the previous
    // briefing's paste marker offers no proof that THIS send arrived, so it must
    // not license a keystroke — otherwise stale scrollback could mask a fresh
    // dialog and re-create the exact false-Enter failure this check exists to
    // stop.
    #[test]
    fn send_does_not_nudge_on_evidence_that_predates_the_prompt() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-stale-evidence-test");

        phase_start(&h, &mut run, "code", None).unwrap();
        h.push_status(Some("idle"));
        // The paste marker is already there BEFORE the prompt, and still there
        // after — unchanged, so it proves nothing about this send.
        let stale = "> [Pasted text #1 +40 lines]\n  Composer 2.5";
        h.push_read(stale);
        h.push_read(stale);
        h.push_outcome(PromptOutcome::Stalled);

        let err = phase_send_with_timeout(
            &h,
            &mut run,
            "code",
            "hello agent",
            Duration::from_secs(5),
            Duration::from_millis(1),
            CONFIRM,
        )
        .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(
            err.to_string().contains("was NOT delivered"),
            "stale evidence must read as undelivered, not as a stuck composer: {err}"
        );
        // And it must not tell the human the payload is "nowhere in the composer"
        // while a paste marker is sitting there in plain sight — that is the same
        // class of confidently-wrong diagnosis this change exists to remove.
        assert!(
            !err.to_string().contains("nowhere in the agent's composer"),
            "the swallow narrative is false when the composer visibly holds a \
             payload signature; say it is stale instead: {err}"
        );
        assert!(
            err.to_string().contains("before this prompt"),
            "must explain that the evidence predates the send: {err}"
        );
        assert!(
            !h.calls().iter().any(|c| c.contains("agent_send_keys")),
            "must not nudge on evidence that predates the prompt: {:?}",
            h.calls()
        );
    }

    // A pane we cannot read is a pane we cannot reason about: with no evidence to
    // weigh, `phase_send` must fail safe (raise, no keystroke) rather than nudging
    // blind into an unknown screen.
    #[test]
    fn send_raises_without_keystroke_when_pane_is_unreadable() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-unreadable-test");

        phase_start(&h, &mut run, "code", None).unwrap();
        h.push_status(Some("idle"));
        h.fail_agent_read();
        h.push_outcome(PromptOutcome::Stalled);

        let err = phase_send_with_timeout(
            &h,
            &mut run,
            "code",
            "hello agent",
            Duration::from_secs(5),
            Duration::from_millis(1),
            CONFIRM,
        )
        .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(
            !h.calls().iter().any(|c| c.contains("agent_send_keys")),
            "must not nudge a pane it cannot read: {:?}",
            h.calls()
        );
        // "Could not look" is a different fact from "looked, found nothing", and
        // they send the human somewhere different: one is a herdr problem, the
        // other is a dialog on the screen. Asserting the swallow narrative here
        // would be a confident diagnosis drovr has no evidence for.
        assert!(
            !err.to_string().contains("nowhere in the agent's composer"),
            "must not claim the composer is empty when the pane could not be read: {err}"
        );
        assert!(
            err.to_string().contains("could not be READ"),
            "must name the unreadable pane as the reason it will not guess: {err}"
        );
    }

    // The nudge needs evidence that APPEARED, and "appeared" is only knowable if
    // the BEFORE look succeeded. A pane that could not be read before the prompt
    // and shows a paste marker after might have been showing it all along, so the
    // marker is not attributable to this send and must not license a keystroke.
    //
    // This is the fail-safe the earlier `unwrap_or(true)` encoded, and the exact
    // one a three-state refactor can drop by writing `before != Present`.
    #[test]
    fn send_does_not_nudge_when_the_before_look_failed() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-blind-before-test");

        phase_start(&h, &mut run, "code", None).unwrap();
        h.push_status(Some("idle"));
        // The pre-prompt read fails; the post-stall read succeeds and shows a
        // paste marker of unknown age.
        h.fail_agent_read();
        h.push_outcome(PromptOutcome::Stalled);
        h.allow_agent_read_after(1);
        h.push_read("> [Pasted text #1 +40 lines]\n  Composer 2.5");

        let err = phase_send_with_timeout(
            &h,
            &mut run,
            "code",
            "hello agent",
            Duration::from_secs(5),
            Duration::from_millis(1),
            CONFIRM,
        )
        .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(
            !h.calls().iter().any(|c| c.contains("agent_send_keys")),
            "evidence is only fresh if the BEFORE look succeeded: {:?}",
            h.calls()
        );
        // Refusing is right; saying the composer is empty is not. drovr can SEE a
        // payload signature — it just cannot date it — and telling the human to
        // clear the pane sends them past text they could submit by hand.
        assert!(
            !err.to_string().contains("nowhere in the agent's composer"),
            "must not claim an empty composer while the after-read shows a payload: {err}"
        );
        assert!(
            err.to_string()
                .contains("cannot tell whether it is this one"),
            "must say the visible payload cannot be dated, not that it is absent: {err}"
        );
    }

    // A nudge that does not help must not be reported as a delivered seed.
    #[test]
    fn send_raises_when_nudge_does_not_submit() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-stuck-composer-test");

        phase_start(&h, &mut run, "code", None).unwrap();
        h.push_status(Some("idle"));
        h.push_read("> Plan, search, build anything\n  Composer 2.5");
        h.push_read("> [Pasted text #1 +40 lines]\n  Composer 2.5");
        // The prompt stalls, and so does the wait after the Enter.
        h.push_outcome(PromptOutcome::Stalled);
        h.push_outcome(PromptOutcome::Stalled);

        let err = phase_send_with_timeout(
            &h,
            &mut run,
            "code",
            "hello agent",
            Duration::from_secs(5),
            Duration::from_millis(1),
            CONFIRM,
        )
        .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(
            err.to_string().contains("would not submit"),
            "error must name the composer-stuck failure, not the swallow one: {err}"
        );
    }

    // An undelivered seed leaves the SAME wreckage a transport failure does: the
    // re-open already ran, so the phase is Running with its completion marker
    // gone. Saying only "the seed did not arrive" leaves a phantom incomplete
    // phase nobody knows to clean up.
    #[test]
    fn an_undelivered_seed_also_reports_the_re_open_it_left_behind() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-undelivered-reopen-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        write_handoff(&run, "plan");
        let pass = run.phases[0].pass.clone().unwrap();
        agent_signals_done(&run, "plan", &pass);
        run.phases[0].status = PhaseStatus::Done;
        run.save().unwrap();

        h.push_status(Some("idle"));
        h.push_read(CLAUDE_MCP_MODAL_PANE);
        h.push_read(CLAUDE_MCP_MODAL_PANE);
        h.push_outcome(PromptOutcome::Stalled);

        let err = phase_send_with_timeout(
            &h,
            &mut run,
            "plan",
            "next",
            Duration::from_secs(5),
            Duration::from_millis(1),
            CONFIRM,
        )
        .unwrap_err()
        .to_string();

        assert!(
            !done_marker(&run.name, "plan").exists(),
            "precondition: the re-open really did clear the marker"
        );
        assert!(
            err.contains("was NOT delivered"),
            "it must still name the delivery failure: {err}"
        );
        assert!(
            err.contains("completion marker"),
            "and report the state the re-open left behind: {err}"
        );
    }

    // -- agent_has_started: which statuses mean "attached AND at the composer" -
    #[test]
    fn agent_has_started_recognizes_attached_states() {
        // At the composer, safe to send.
        assert!(agent_has_started(Some(&AgentStatus::Idle)));
        assert!(agent_has_started(Some(&AgentStatus::Working)));
        assert!(agent_has_started(Some(&AgentStatus::Done)));
        // Still-booting / unconfirmed: must keep waiting.
        assert!(!agent_has_started(None));
        assert!(!agent_has_started(Some(&AgentStatus::Unknown)));
        // `blocked` = attached but parked on a prompt, NOT at the composer — must
        // NOT release the gate (sending would type into the dialog).
        assert!(!agent_has_started(Some(&AgentStatus::Blocked)));
        // A herdr state this drovr has never seen is not "started" either: an
        // unrecognised status is not evidence of a composer.
        assert!(!agent_has_started(Some(&AgentStatus::Other(
            "compacting".to_string()
        ))));
    }

    // -- phase_send waits for the agent to attach before sending --------------
    #[test]
    fn send_waits_for_agent_ready_before_sending() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-ready-test");

        phase_start(&h, &mut run, "code", None).unwrap();
        // The agent boots: unreadable, then `unknown` (pane seen, no agent yet),
        // then `blocked` (attached but parked on a prompt — still NOT at the
        // composer), then finally a composer-ready state. The send must NOT fire
        // until that last state; in particular `blocked` must not release it.
        h.push_status(None::<String>);
        h.push_status(Some("unknown"));
        h.push_status(Some("blocked"));
        h.push_status(Some("idle"));

        // Tiny poll interval so the three waited polls don't cost real wall-clock.
        phase_send_with_timeout(
            &h,
            &mut run,
            "code",
            "hello agent",
            Duration::from_secs(5),
            Duration::from_millis(1),
            CONFIRM,
        )
        .unwrap();

        let calls = h.calls();
        // Polled through all three un-ready states (incl. blocked) before the send,
        // and every poll came before the send.
        let first_send = calls
            .iter()
            .position(|c| c.contains("agent_prompt_confirm"))
            .unwrap();
        let status_polls = calls
            .iter()
            .filter(|c| c.contains("agent_status"))
            .count();
        assert!(
            status_polls >= 4,
            "must poll through none/unknown/blocked until the composer is ready: {calls:?}"
        );
        assert!(
            calls[..first_send]
                .iter()
                .filter(|c| c.contains("agent_status"))
                .count()
                == status_polls,
            "all status polls must precede the send: {calls:?}"
        );
    }

    // A follow-up send to an already-`working` agent must NOT block: `working` is
    // a started state, so the gate releases on the first poll (regression guard
    // for the "gate on idle only → 30s stall on busy agents" bug).
    #[test]
    fn send_to_working_agent_does_not_block() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-working-test");

        phase_start(&h, &mut run, "code", None).unwrap();
        h.push_status(Some("working"));

        let start = Instant::now();
        phase_send(&h, &mut run, "code", "follow-up").unwrap();
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "send to a working agent must not wait out the readiness timeout"
        );
        // Exactly one status poll: the first `working` releases the gate.
        let calls = h.calls();
        assert_eq!(
            calls.iter().filter(|c| c.contains("agent_status")).count(),
            1,
            "a started agent must release the gate on the first poll: {calls:?}"
        );
    }

    // When the agent never attaches within the readiness timeout, phase_send must
    // RAISE (a `TimedOut` error naming the run/phase) and must NOT send — a prompt
    // into a never-ready pane would be silently swallowed.
    #[test]
    fn send_raises_and_does_not_send_when_agent_never_ready() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-never-ready-test");

        phase_start(&h, &mut run, "code", None).unwrap();
        // The pane keeps reporting an un-started state — it never attaches. (Enough
        // entries to outlast every poll within the tiny timeout below; the default
        // is `idle`, so the queue must not drain to it before the deadline.)
        for _ in 0..8 {
            h.push_status(Some("unknown"));
        }

        // Coarse poll interval (500ms) vs a 50ms timeout ⇒ at most 2 polls, so the
        // 8 queued `unknown`s can never drain to the idle default before the
        // deadline. Deterministic, ~50ms wall-clock.
        let err = phase_send_with_timeout(
            &h,
            &mut run,
            "code",
            "seed text",
            Duration::from_millis(50),
            POLL_INTERVAL,
            CONFIRM,
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut, "must be a TimedOut error");
        assert!(
            err.to_string().contains("send-never-ready-test")
                && err.to_string().contains("code"),
            "error must name the run and phase: {err}"
        );
        assert!(
            err.to_string().contains("drovr attach"),
            "error must suggest attach: {err}"
        );
        // Crucially, no prompt was delivered.
        assert!(
            !h.calls().iter().any(|c| c.contains("agent_prompt_confirm")),
            "must not send a prompt when the agent never became ready: {:?}",
            h.calls()
        );
    }

    #[test]
    fn start_with_seed_records_handoff_doc() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("seed-test");
        let seed = Path::new("/tmp/seed.md");

        phase_start(&h, &mut run, "brainstorm", Some(seed)).unwrap();

        // (a) handoff_doc stores the seed path for later injection via agent_send
        let p = &run.phases[0];
        assert_eq!(p.handoff_doc.as_deref(), Some("/tmp/seed.md"));
        // (b) the launch command must NOT contain "--seed" or the seed path —
        //     seed injection happens via the first agent_send, not the launch command
        let calls = h.calls();
        let run_call = calls.iter().find(|c| c.contains("pane_run")).unwrap();
        assert!(
            !run_call.contains("--seed"),
            "command must not contain --seed: {run_call}"
        );
        assert!(
            !run_call.contains("/tmp/seed.md"),
            "command must not contain seed path: {run_call}"
        );
    }

    // A launch closes exactly the panes it SUPERSEDES — every other phase that
    // is `Done` and still holds one (`phase::reap_tests`) — and nothing else.
    // Here both phases are `Running`, so there is nothing to supersede and the
    // count is zero: a launch must never close a pane that is still in play, and
    // never the one it is launching into.
    #[test]
    fn phase_start_never_closes_a_pane_that_is_still_in_play() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("no-mid-run-close-test");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();
        phase_start(&h, &mut run, "plan", None).unwrap();

        let calls = h.calls();
        assert!(
            !calls.iter().any(|c| c.contains("pane_close")),
            "a launch must not close a pane of a phase that is still running: {calls:?}"
        );
        assert!(
            !calls.iter().any(|c| c.contains("agent_start")),
            "phase_start must not use agent_start: {calls:?}"
        );
    }

    #[test]
    fn start_reuses_existing_phase_entry() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("reuse-test");

        // Pre-populate a Pending phase
        run.phases.push(Phase::new("plan"));

        phase_start(&h, &mut run, "plan", None).unwrap();
        // Still only one phase
        assert_eq!(run.phases.len(), 1);
        assert_eq!(run.phases[0].status, PhaseStatus::Running);
    }

    #[test]
    fn phase_start_clears_stale_done_marker() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("stale-marker-test");

        // Pass 1: the phase runs and signals done, leaving `<phase>.done` behind.
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pass_1 = run.phases[0].pass.clone().unwrap();
        write_handoff(&run, "plan");
        agent_signals_done(&run, "plan", &pass_1);
        let marker = done_marker(&run.name, "plan");
        assert!(marker.exists());

        // Pass 2: the driver re-enters the phase. Nothing else sweeps the marker,
        // so if `phase_start` leaves it the next `phase wait` returns `Done`
        // instantly off the PREVIOUS pass's marker and the phase is never awaited.
        phase_start(&h, &mut run, "plan", None).unwrap();
        assert!(
            !marker.exists(),
            "phase_start must delete the stale done marker at {}",
            marker.display()
        );
        assert_eq!(run.phases[0].status, PhaseStatus::Running);

        // Drive the phase to a genuine `Done` in state, so the pass-3 assertions
        // below actually exercise the reset (nothing else writes `Done`).
        agent_signals_done(&run, "plan", &run.phases[0].pass.clone().unwrap());
        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::Done
        );
        assert_eq!(run.phases[0].status, PhaseStatus::Done);

        // Pass 3, with the launch scripted to fail: the marker must STILL be gone.
        // (Also proves the delete is not skipped when the launch errors.)
        // That pins the delete AHEAD of `launch_in_pane` — the previous pass's
        // agent is still alive and can drop a marker at any moment, so deleting
        // late leaves a wider window in which `phase wait` short-circuits on it.
        std::fs::write(&marker, b"").unwrap();
        let failing = FakeHerdr::new();
        failing.fail_pane_run();
        assert!(phase_start(&failing, &mut run, "plan", None).is_err());
        assert!(
            !marker.exists(),
            "the stale marker must be cleared before the launch, not after it"
        );
        // ...and the phase must not be left reporting the PREVIOUS pass's
        // completion. `phase_wait` short-circuits on a `Done` status, so a failed
        // re-launch that left `Done` behind would report success for a phase with
        // no agent running at all.
        assert_ne!(
            run.phases[0].status,
            PhaseStatus::Done,
            "a failed re-launch must not leave the previous pass's Done status"
        );
        // The reset persists the NEW token rather than clearing it: a phase whose
        // token no agent holds rejects every marker (fail-closed). Clearing to
        // `None` would drop it into the legacy "accept any marker" bucket while
        // the previous pass's agent is still alive — fail-open, the wrong way.
        let after = run.phases[0].pass.clone().expect("a started phase always has a token");
        assert_ne!(
            after, pass_1,
            "a re-launch must not leave the PREVIOUS pass's token"
        );
    }

    // -- pass tokens: a re-entered phase must not complete off the old pass ------
    //
    // The pre-launch marker delete only narrows the window. The PREVIOUS pass's
    // agent is still alive in the reused pane — a re-entry relaunches into the
    // same pane, and a launch never reaps the phase it is re-entering — and can
    // run `drovr phase done` again at any moment, recreating the marker after the
    // delete. Every test below drives that exact sequence.

    /// Simulate the agent launched by pass `token` running `drovr phase done`:
    /// the pane's environment carries `DROVR_PASS`, so the marker it writes is
    /// stamped with that pass's token.
    fn agent_signals_done(run: &RunState, phase: &str, token: &PassToken) {
        unsafe {
            std::env::set_var(PASS_ENV, token.as_str());
        }
        phase_done(run, phase).unwrap();
        unsafe {
            std::env::remove_var(PASS_ENV);
        }
    }

    /// A saved run with `plan` registered as a pipeline phase — the shape `phase_done`'s
    /// handoff gate applies to.
    fn registered_phase_run(name: &str) -> RunState {
        let mut run = make_run(name);
        run.phases.push({
                let mut p = Phase::new("plan");
                p.status = PhaseStatus::Running;
                p
            }
            .with_pane("p1"));
        run.save().unwrap();
        run
    }

    fn write_raw_handoff(run: &RunState, phase: &str, body: &str) {
        let hp = run_dir(&run.name).join(format!("{phase}-HANDOFF.md"));
        std::fs::create_dir_all(hp.parent().unwrap()).unwrap();
        std::fs::write(&hp, body).unwrap();
    }

    fn write_handoff(run: &RunState, phase: &str) {
        let hp = run_dir(&run.name).join(format!("{phase}-HANDOFF.md"));
        std::fs::create_dir_all(hp.parent().unwrap()).unwrap();
        std::fs::write(&hp, "## Objective\nreal handoff\n").unwrap();
    }

    #[test]
    fn phase_wait_rejects_a_marker_recreated_by_the_previous_pass() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("stale-pass-test");

        // Pass 1 runs to completion.
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pass_a = run.phases[0].pass.clone().expect("pass 1 must mint a token");
        write_handoff(&run, "plan");
        agent_signals_done(&run, "plan", &pass_a);

        // Pass 2 re-enters the phase: fresh token, stale marker swept.
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pass_b = run.phases[0].pass.clone().expect("pass 2 must mint a token");
        assert_ne!(pass_a, pass_b, "each pass must mint a distinct token");
        assert!(!done_marker(&run.name, "plan").exists());

        // THE RACE: pass 1's agent is still alive and signals done again. Its
        // environment was fixed at ITS launch, so it holds pass 1's token — no
        // state change can make it hold pass 2's.
        //
        // First line of defence: `phase_done` refuses outright, so the agent is
        // told it did NOT complete the phase rather than exiting 0 on a marker
        // that could never be honoured.
        unsafe {
            std::env::set_var(PASS_ENV, pass_a.as_str());
        }
        let err = phase_done(&run, "plan").unwrap_err();
        unsafe {
            std::env::remove_var(PASS_ENV);
        }
        assert!(
            err.to_string().contains("could never be matched to the running pass"),
            "the stale agent must be refused at the source: {err}"
        );
        assert!(!done_marker(&run.name, "plan").exists());

        // Second line of defence, and the one that matters if such a marker exists
        // anyway — it predates the re-entry, or state was reverted by a lost
        // update. Write it directly with pass 1's token.
        let marker = done_marker(&run.name, "plan");
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, pass_a.as_str()).unwrap();

        // The driver must NOT be told pass 2 finished.
        let out = phase_wait(&h, &mut run, "plan", 50).unwrap();
        assert_eq!(
            out,
            PhaseWaitOutcome::TimedOut,
            "a marker from a previous pass must never complete the current one"
        );
        assert_eq!(
            run.phases[0].status,
            PhaseStatus::Running,
            "the phase must stay Running so the driver keeps awaiting the live agent"
        );
    }

    #[test]
    fn phase_wait_completes_from_marker_evidence_and_keeps_it() {
        // The marker is the EVIDENCE of completion; the recorded `Done` status is
        // only a cache of it. `phase_wait` therefore derives its verdict from the
        // marker every time and never consumes it — which is also what keeps a
        // repeated wait idempotent WITHOUT a `status == Done` short-circuit. That
        // short-circuit is what made every "state persisted but marker destroyed"
        // ordering hazard reportable as a false completion.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("current-pass-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        let pass = run.phases[0].pass.clone().unwrap();
        write_handoff(&run, "plan");
        agent_signals_done(&run, "plan", &pass);

        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::Done
        );
        assert_eq!(run.phases[0].status, PhaseStatus::Done);
        assert!(
            done_marker(&run.name, "plan").exists(),
            "the marker is the evidence and must be retained"
        );
        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::Done,
            "re-waiting must stay idempotent, off the marker rather than the status"
        );
    }

    #[test]
    fn phase_start_persists_the_new_pass_before_destroying_the_old_marker() {
        // Finding 1. Both steps can fail, and no failure may leave a phase that
        // looks complete with no agent running. Persist-then-sweep is the safe
        // order here: if the save fails, NOTHING has been touched, so the old
        // status and old marker stay mutually consistent and describe a pass that
        // genuinely did finish. Sweep-first would destroy the marker and then fail
        // to record the new pass — leaving a bare `Done` on disk.
        //
        // Isolating a save failure from a sweep failure: make `state.json` a
        // DIRECTORY. `save`'s final rename onto it fails, while `remove_file` on
        // the marker would still succeed — so the marker's survival is a direct
        // observation of which step ran first.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("start-order-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        let pass = run.phases[0].pass.clone().unwrap();
        write_handoff(&run, "plan");
        agent_signals_done(&run, "plan", &pass);
        let marker = done_marker(&run.name, "plan");
        assert!(marker.exists());

        let state = run_dir(&run.name).join("state.json");
        std::fs::remove_file(&state).unwrap();
        std::fs::create_dir(&state).unwrap();

        let res = phase_start(&h, &mut run, "plan", None);
        std::fs::remove_dir(&state).unwrap();

        assert!(res.is_err(), "an unwritable state.json must fail phase_start");
        assert!(
            marker.exists(),
            "the completion marker must survive a failed state write — destroying \
             it first would leave the phase Done on disk with no agent running"
        );
    }

    #[test]
    fn supersession_guard_fails_closed_when_state_cannot_be_reread() {
        // Finding 3. The guard used `if let Ok(fresh) = RunState::load(..)`, so any
        // load failure SKIPPED the check and the waiter completed anyway. A
        // completion that cannot be verified must never be reported: reaping tears
        // panes down on this verdict, so an unverifiable Done closes a live pane.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("guard-fail-closed-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        let pass = run.phases[0].pass.clone().unwrap();
        write_handoff(&run, "plan");
        agent_signals_done(&run, "plan", &pass);

        // Make the re-read fail while leaving the (matching) marker in place.
        std::fs::remove_file(run_dir(&run.name).join("state.json")).unwrap();

        let err = phase_wait(&h, &mut run, "plan", 50)
            .expect_err("an unverifiable completion must fail, not be reported as Done");
        assert!(
            err.to_string().contains("Refusing to report done"),
            "the error must say why it refused: {err}"
        );
    }

    #[test]
    fn phase_wait_leaves_the_caller_holding_the_persisted_state() {
        // `phase_wait` commits onto freshly-loaded state. If it then patched only
        // `status` into the caller's entry-time snapshot, a caller that saves after
        // waiting — reaping records the reap exactly that way — would write the
        // stale snapshot back and undo everything else that happened meanwhile.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("caller-state-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        let pass = run.phases[0].pass.clone().unwrap();
        write_handoff(&run, "plan");

        // Another process advances the run while this waiter holds its snapshot.
        let mut other = RunState::load("caller-state-test").unwrap();
        other.cursor = 7;
        other.gate = "moved-on".into();
        other.save().unwrap();
        assert_eq!(run.cursor, 0, "the waiter's snapshot is stale by construction");

        agent_signals_done(&run, "plan", &pass);
        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::Done
        );

        assert_eq!(run.cursor, 7, "the caller must hold what was persisted");
        assert_eq!(run.gate, "moved-on");
        assert_eq!(run.phases[0].status, PhaseStatus::Done);
        // And saving the caller's copy must not undo the other process's work.
        run.save().unwrap();
        let on_disk = RunState::load("caller-state-test").unwrap();
        assert_eq!(on_disk.cursor, 7);
        assert_eq!(on_disk.phases[0].status, PhaseStatus::Done);
    }

    #[test]
    fn phase_wait_fails_closed_when_the_phase_vanishes_from_fresh_state() {
        // "Phase absent" must stay distinct from "phase present with no token".
        // Flattening them (`and_then`) makes a vanished LEGACY phase compare
        // None == None, pass the supersession guard, write nothing, and still
        // return Done — a false completion with no persisted trace.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("vanished-phase-test");
        run.phases.push(
            {
                let mut p = Phase::new("plan");
                p.status = PhaseStatus::Running;
                p
            }
            .with_pane("p1"),
        );
        assert!(run.phases[0].pass.is_none(), "legacy phase: no token");
        run.save().unwrap();
        write_handoff(&run, "plan");
        let marker = done_marker(&run.name, "plan");
        std::fs::write(&marker, b"").unwrap(); // legacy marker: accepted by the rule

        // Another writer drops the phase from state (a stale-snapshot save-back).
        let mut clobbered = RunState::load("vanished-phase-test").unwrap();
        clobbered.phases.clear();
        clobbered.save().unwrap();

        let err = phase_wait(&h, &mut run, "plan", 50)
            .expect_err("a phase missing from fresh state must not be reported done");
        assert!(
            err.to_string().contains("no longer present in the run state"),
            "the error must say why it refused: {err}"
        );
    }

    #[test]
    fn phase_wait_refuses_a_superseded_passs_completion() {
        // A wait can run for an hour. If a `phase start` re-entered the phase
        // meanwhile, the marker this waiter matches belongs to a dead pass.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("superseded-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        let pass_a = run.phases[0].pass.clone().unwrap();
        write_handoff(&run, "plan");

        // Another process re-enters the phase: new token, `Running`, persisted.
        let mut other = RunState::load("superseded-test").unwrap();
        phase_start(&h, &mut other, "plan", None).unwrap();
        let pass_b = other.phases[0].pass.clone().unwrap();
        assert_ne!(pass_a, pass_b);

        // Pass A's still-live agent now signals done with ITS token.
        agent_signals_done(&run, "plan", &pass_a);

        // `run` is this waiter's hour-old snapshot, still expecting pass A.
        // Supersession is its OWN outcome, never `TimedOut`: "another pass took
        // over, all is well" and "the agent is stuck" are opposite verdicts, and
        // reaping tears panes down on this one.
        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::Superseded,
            "a superseded pass's completion must not be reported"
        );
        // ...and the re-entry must survive: not clobbered back to Done/pass A.
        let on_disk = RunState::load("superseded-test").unwrap();
        assert_eq!(on_disk.phases[0].status, PhaseStatus::Running);
        assert_eq!(on_disk.phases[0].pass, Some(pass_b));
    }

    #[test]
    fn a_superseded_wait_leaves_the_caller_holding_the_re_entrys_state() {
        // The clobber half of the same defect. `phase_wait`'s Done path adopts the
        // freshly loaded state (`*run = fresh`) precisely so a caller that saves
        // after waiting cannot write an hour-old snapshot back; reaping records a
        // reap exactly that way. The supersession path has the SAME hazard and a
        // worse payload: the waiter's snapshot still carries the superseded pass
        // token and its `Running`/`Done` status, so saving it would restore the
        // dead pass and undo the very re-entry that superseded this wait.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("superseded-no-clobber-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        let pass_a = run.phases[0].pass.clone().unwrap();
        write_handoff(&run, "plan");

        let mut other = RunState::load("superseded-no-clobber-test").unwrap();
        phase_start(&h, &mut other, "plan", None).unwrap();
        let pass_b = other.phases[0].pass.clone().unwrap();
        assert_ne!(pass_a, pass_b);

        agent_signals_done(&run, "plan", &pass_a);

        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::Superseded
        );
        // The caller's snapshot must already BE the fresh state, so that the
        // save-after-wait a reaping caller performs is a no-op, not a rollback.
        assert_eq!(
            run.phases[0].pass,
            Some(pass_b.clone()),
            "the waiter must adopt the fresh state, not keep the superseded snapshot"
        );
        run.save().unwrap();
        let on_disk = RunState::load("superseded-no-clobber-test").unwrap();
        assert_eq!(
            on_disk.phases[0].pass,
            Some(pass_b),
            "a save after a superseded wait must not restore the dead pass"
        );
        assert_eq!(on_disk.phases[0].status, PhaseStatus::Running);
    }

    #[test]
    fn a_superseded_wait_says_so_even_when_no_marker_ever_lands() {
        // The COMMON supersession shape, and the one the driver actually hit: a
        // `phase start` re-enters the phase, the superseded agent never signals
        // anything, and this waiter just sits there. Detecting supersession only
        // when a stale marker happens to land would leave that case reporting
        // `TimedOut` — "the agent is stuck" — for a phase that is perfectly
        // healthy under a newer pass. The whole point of the distinct outcome is
        // that a caller never has to guess which of the two it is got.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("superseded-no-marker-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        let pass_a = run.phases[0].pass.clone().unwrap();

        let mut other = RunState::load("superseded-no-marker-test").unwrap();
        phase_start(&h, &mut other, "plan", None).unwrap();
        let pass_b = other.phases[0].pass.clone().unwrap();
        assert_ne!(pass_a, pass_b);

        // No marker on disk at all: nothing has completed, and this waiter's pass
        // no longer exists.
        assert!(!done_marker(&run.name, "plan").exists());
        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::Superseded,
            "a wait that outlived its own pass is superseded, not stuck"
        );
        // Same no-clobber requirement as the marker path.
        assert_eq!(run.phases[0].pass, Some(pass_b));
    }

    #[test]
    fn pass_drift_separates_a_newer_pass_from_a_lost_token() {
        let a = PassToken::new("a".into()).unwrap();
        let b = PassToken::new("b".into()).unwrap();
        // The same pass, tokened or legacy.
        assert_eq!(PassDrift::between(Some(&a), Some(&a)), PassDrift::Same);
        assert_eq!(PassDrift::between(None, None), PassDrift::Same);
        // A different token is a re-entry.
        assert_eq!(PassDrift::between(Some(&b), Some(&a)), PassDrift::Superseded);
        // A LEGACY phase that has since acquired a token: `phase_start` is the only
        // thing that mints one, so this is a re-entry by a token-minting build —
        // the mixed-era case, and the reason this arm is not folded into `Same`.
        assert_eq!(PassDrift::between(Some(&a), None), PassDrift::Superseded);
        // The one direction that is NOT a re-entry: nothing mints `None`.
        assert_eq!(PassDrift::between(None, Some(&a)), PassDrift::TokenLost);
    }

    #[test]
    fn a_dropped_pass_token_is_not_a_supersession() {
        // The false positive a self-review caught. Task 1's handoff §5.6: an OLDER
        // drovr on `PATH` does not know the `pass` field, so any save it performs
        // silently drops it — `Some(x)` → `None` on disk with NO re-entry. A bare
        // `!=` reads that as supersession and tells the driver "nothing is wrong,
        // re-run the wait", which then hangs forever (an untokened phase only
        // accepts an EMPTY marker, and the live agent still stamps its token).
        //
        // Supersession means a NEWER pass exists. `phase_start` always mints
        // `Some`, so a token that VANISHED is corruption, not a re-entry, and it
        // must stay a timeout — the loud, diagnosable outcome — never "all is well".
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("dropped-token-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        let pass_a = run.phases[0].pass.clone().unwrap();
        write_handoff(&run, "plan");

        // An older binary re-saves the run, dropping the field it cannot see.
        let mut older = RunState::load("dropped-token-test").unwrap();
        older.phases[0].pass = None;
        older.save().unwrap();

        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::TimedOut,
            "a token that vanished is corruption, not another pass taking over"
        );
        // And the caller must NOT adopt the corrupted state: its snapshot still
        // holds the token, which is what makes recovery possible at all.
        assert_eq!(run.phases[0].pass, Some(pass_a.clone()));

        // Same on the marker path: the live agent's own marker lands, matching the
        // token this waiter holds, while the phase on disk has lost it. Not Done
        // (the fresh state cannot account for the token), and not Superseded.
        agent_signals_done(&run, "plan", &pass_a);
        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::TimedOut,
            "a matching marker against a phase whose token vanished is not a completion"
        );
    }

    #[test]
    fn a_timeout_that_cannot_re_read_state_stays_a_timeout() {
        // The deadline check fails OPEN, the opposite of the marker path's guard,
        // and that asymmetry is deliberate: there the question is "may I report a
        // completion I cannot verify" (no), here the conservative answer already IS
        // `TimedOut`. If someone later "aligns" this with the fail-closed guard,
        // an unreadable state file turns an honest timeout into exit 1 — which the
        // handoff skill documents as STOP — and breaks the re-arm loop.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("timeout-unreadable-state-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        std::fs::write(run_dir(&run.name).join("state.json"), b"{ not json").unwrap();

        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::TimedOut,
            "an unverifiable timeout is still a timeout, never an error"
        );
    }

    #[test]
    fn a_timeout_whose_phase_vanished_stays_a_timeout() {
        // Mirror image of `phase_wait_fails_closed_when_the_phase_vanishes_from
        // _fresh_state`, with the opposite (open) failure mode for the same reason:
        // a vanished phase is not evidence of a re-entry, and nothing is being
        // reported complete here.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("timeout-vanished-phase-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        let mut clobbered = RunState::load("timeout-vanished-phase-test").unwrap();
        clobbered.phases.clear();
        clobbered.save().unwrap();

        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::TimedOut
        );
    }

    #[test]
    fn a_timeout_on_the_current_pass_is_still_a_timeout() {
        // The other side of the same check: supersession must not become a
        // catch-all for "the wait ended without a marker". A phase still running
        // the pass this waiter expects has to keep reporting `TimedOut`, or the
        // driver stops triaging genuinely stuck agents.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("timeout-still-timeout-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::TimedOut
        );
    }

    #[test]
    fn a_stale_done_status_alone_never_completes_a_phase() {
        // The root fix, stated directly. Every ordering hazard in `phase_start` /
        // `phase_send` has the same shape: the marker gets destroyed and the
        // status write does not land (or vice versa), leaving `Done` on disk with
        // no agent running. A status short-circuit would report success there.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("stale-done-status-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        // Hand-craft exactly the wreckage those failure paths leave behind.
        run.phases[0].status = PhaseStatus::Done;
        run.save().unwrap();
        assert!(!done_marker(&run.name, "plan").exists());

        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::TimedOut,
            "a Done status with no marker is not evidence of completion"
        );
    }

    #[test]
    fn send_to_a_finished_phase_reopens_it_so_the_next_wait_actually_waits() {
        // The pipeline's DOCUMENTED re-entry (skills/pipeline/SKILL.md): after an
        // exit-3 review iteration the driver runs `phase send` — NOT `phase start`
        // — into the still-live agent, then `phase wait`. If the send does not
        // re-open the phase, the previous iteration's Done status and marker both
        // survive and that wait returns Done in microseconds while the agent has
        // not even read the prompt. The driver then advances (and, once reaping
        // lands, tears down the pane it just messaged).
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-reentry-test");

        phase_start(&h, &mut run, "implement-task-1", None).unwrap();
        let pass = run.phases[0].pass.clone().unwrap();
        write_handoff(&run, "implement-task-1");
        agent_signals_done(&run, "implement-task-1", &pass);
        assert_eq!(
            phase_wait(&h, &mut run, "implement-task-1", 50).unwrap(),
            PhaseWaitOutcome::Done
        );

        // The still-live agent drops ANOTHER marker (it re-ran `phase done`, or
        // simply finished again) after the earlier one was accepted. Without this
        // the sweep assertion below would hold vacuously — `phase_wait` retains the
        // marker it accepts.
        agent_signals_done(&run, "implement-task-1", &pass);
        assert!(done_marker(&run.name, "implement-task-1").exists());

        // Exit 3: forward the findings to the SAME live agent.
        h.push_status(Some("idle"));
        phase_send(&h, &mut run, "implement-task-1", "fix every finding").unwrap();
        assert_eq!(
            run.phases[0].status,
            PhaseStatus::Running,
            "sending to a finished phase must re-open it"
        );
        assert!(
            !done_marker(&run.name, "implement-task-1").exists(),
            "the previous iteration's marker must be swept on re-entry"
        );

        // The wait that follows must now actually wait.
        assert_eq!(
            phase_wait(&h, &mut run, "implement-task-1", 50).unwrap(),
            PhaseWaitOutcome::TimedOut,
            "the wait after a send must not report the PREVIOUS iteration's completion"
        );

        // The same agent (same pane, same token) finishing the fix completes it.
        agent_signals_done(&run, "implement-task-1", &pass);
        assert_eq!(
            phase_wait(&h, &mut run, "implement-task-1", 50).unwrap(),
            PhaseWaitOutcome::Done
        );
    }

    #[test]
    fn send_sweeps_a_marker_even_when_the_phase_is_still_running() {
        // The gap a `status == Done` gate would leave open. A marker sits on disk
        // with a MATCHING token for the whole interval between "the agent wrote
        // it" and "some phase_wait consumed it" — and if no wait was running, that
        // interval is unbounded, with the status still `Running`. A send that
        // skipped the sweep in that state would be followed by a wait that
        // completes instantly off work finished before the send was even issued.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-running-sweep-test");

        phase_start(&h, &mut run, "implement-task-1", None).unwrap();
        let pass = run.phases[0].pass.clone().unwrap();
        write_handoff(&run, "implement-task-1");
        // The agent finishes, but NOBODY waits — status stays Running.
        agent_signals_done(&run, "implement-task-1", &pass);
        assert_eq!(run.phases[0].status, PhaseStatus::Running);
        assert!(done_marker(&run.name, "implement-task-1").exists());

        h.push_status(Some("idle"));
        phase_send(&h, &mut run, "implement-task-1", "more work").unwrap();
        assert!(
            !done_marker(&run.name, "implement-task-1").exists(),
            "a send must sweep the marker even when the phase is still Running"
        );
        assert_eq!(
            phase_wait(&h, &mut run, "implement-task-1", 50).unwrap(),
            PhaseWaitOutcome::TimedOut,
            "the wait after a send must not complete off work that predates it"
        );
    }

    #[test]
    fn failed_send_preserves_the_previous_completion() {
        // The sweep is destructive, so it must happen only once the send is known
        // deliverable. An agent parked on a permission prompt never becomes ready;
        // sweeping before that gate would discard a genuine completion (marker AND
        // Done status) without any new work having been requested.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("failed-send-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        let pass = run.phases[0].pass.clone().unwrap();
        write_handoff(&run, "plan");
        agent_signals_done(&run, "plan", &pass);
        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::Done
        );
        agent_signals_done(&run, "plan", &pass); // agent re-signals; nobody consumes

        // The pane never attaches → the send fails. (Same idiom as
        // `send_raises_and_does_not_send_when_agent_never_ready`: enough queued
        // `unknown`s that the queue cannot drain to FakeHerdr's `idle` default
        // within the tiny timeout.)
        for _ in 0..8 {
            h.push_status(Some("unknown"));
        }
        let err = phase_send_with_timeout(
            &h,
            &mut run,
            "plan",
            "text",
            Duration::from_millis(50),
            POLL_INTERVAL,
            CONFIRM,
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert_eq!(
            run.phases[0].status,
            PhaseStatus::Done,
            "a failed send must not re-open the phase"
        );
        assert!(
            done_marker(&run.name, "plan").exists(),
            "a failed send must not destroy the previous completion marker"
        );
    }

    #[test]
    fn phase_wait_ignores_an_untokenized_marker_for_a_tokened_phase() {
        // An EMPTY marker must not complete a phase that has a token. Accepting it
        // would reopen the race twice: through `fs::write`'s truncate window, and
        // for an agent launched by a PREVIOUS BUILD of drovr (no DROVR_PASS at
        // all), whose marker would otherwise complete a pass it knows nothing
        // about. The documented flow has the agent run `phase done` from inside
        // its own pane, where the token is always present.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("untokenized-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        assert!(run.phases[0].pass.is_some());
        write_handoff(&run, "plan");
        // `phase_done` now refuses rather than writing a marker that could never
        // be accepted — the agent must not be told it succeeded.
        let err = phase_done(&run, "plan").unwrap_err();
        assert!(
            err.to_string().contains("DROVR_PASS=") && err.to_string().contains("drovr phase done"),
            "the refusal must name the explicit-token escape hatch: {err}"
        );
        // Write the empty marker anyway (a pre-token agent would) and confirm
        // `phase_wait` still refuses it.
        let marker = done_marker(&run.name, "plan");
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, b"").unwrap();

        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::TimedOut,
            "an untokenized marker must not complete a tokened phase"
        );
        // Ignored, NOT deleted: unlinking another pass's marker lets a leftover
        // waiter destroy the real completion signal.
        assert!(
            done_marker(&run.name, "plan").exists(),
            "a rejected marker must be left alone, not unlinked"
        );
    }

    #[test]
    fn legacy_phase_completes_only_on_an_untokenized_marker() {
        // Back-compat: a run whose state.json predates pass tokens has
        // `pass: None`, and its live agent has no $DROVR_PASS either — so it can
        // only ever write an EMPTY marker. Accepting that is what stops such a run
        // hanging forever.
        //
        // But `None` must NOT mean "accept anything". A marker bearing a token
        // against a phase that has none is an inconsistency — it means some pass
        // wrote state that was subsequently lost — and trusting it would make the
        // legacy path a general fail-open hole rather than a bounded one.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("legacy-pass-test");
        run.phases.push(
            {
                let mut p = Phase::new("plan");
                p.status = PhaseStatus::Running;
                p
            }
            .with_pane("p1"),
        );
        assert!(run.phases[0].pass.is_none());
        run.save().unwrap(); // phase_wait re-reads state to verify supersession
        write_handoff(&run, "plan");

        // A tokened marker against an untokened phase: rejected.
        let marker = done_marker(&run.name, "plan");
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, b"some-token-from-anywhere").unwrap();
        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::TimedOut,
            "a tokened marker must not complete a phase that has no token"
        );

        // The untokenized marker a genuine pre-token agent writes: accepted.
        std::fs::write(&marker, b"").unwrap();
        assert_eq!(
            phase_wait(&h, &mut run, "plan", 50).unwrap(),
            PhaseWaitOutcome::Done
        );
    }






    /// A handoff written by hand, with no `## ` sections at all, is not scaffolded and must
    /// not be refused by a gate about scaffold sections.
    #[test]
    fn a_sectionless_hand_written_handoff_still_completes() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = registered_phase_run("handoff-hand-written");
        write_raw_handoff(
            &run,
            "plan",
            "Ported the parser to the new lexer; interfaces unchanged. Next agent: run the \
             conformance suite before touching codegen.\n",
        );

        phase_done(&run, "plan").expect("prose without headings is a handoff too");
    }











    /// The accident the gate exists for: run `handoff-scaffold`, forget, signal done.
    #[test]
    fn an_untouched_scaffold_is_refused() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = registered_phase_run("handoff-untouched");
        write_raw_handoff(&run, "plan", &crate::brief::handoff_scaffold());

        let err = phase_done(&run, "plan").unwrap_err().to_string();
        assert!(
            err.contains("nothing you wrote"),
            "must say the file is all scaffold: {err}"
        );
    }

    /// Deleting the placeholders is not writing a handoff either — every remaining line is
    /// still drovr's.
    #[test]
    fn a_scaffold_with_the_placeholders_deleted_is_refused() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = registered_phase_run("handoff-placeholders-deleted");
        let stripped: String = crate::brief::handoff_scaffold()
            .lines()
            .filter(|l| l.trim() != crate::brief::SCAFFOLD_PLACEHOLDER)
            .collect::<Vec<_>>()
            .join("\n");
        write_raw_handoff(&run, "plan", &stripped);

        let err = phase_done(&run, "plan").unwrap_err().to_string();
        assert!(err.contains("nothing you wrote"), "{err}");
    }

    /// Rearranging drovr's own text — fencing the guidance, repeating a heading — is not
    /// writing either.
    #[test]
    fn rearranged_scaffold_text_is_not_writing() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = registered_phase_run("handoff-rearranged");
        let scaffold = crate::brief::handoff_scaffold();
        let lines: Vec<&str> = scaffold
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let body = format!("{}\n```\n{}\n```\n", lines.join("\n"), lines[0]);
        write_raw_handoff(&run, "plan", &body);

        let err = phase_done(&run, "plan").unwrap_err().to_string();
        assert!(err.contains("TODO") || err.contains("nothing you wrote"), "{err}");
    }

    /// A partly-written handoff is refused, and the message names exactly the sections that
    /// still hold the placeholder.
    #[test]
    fn a_partly_written_handoff_names_the_sections_still_holding_todo() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = registered_phase_run("handoff-partly-written");
        let filled = crate::brief::handoff_scaffold().replacen(
            &format!("\n{}\n", crate::brief::SCAFFOLD_PLACEHOLDER),
            "\nPorted the parser to the new lexer; interfaces unchanged.\n",
            1,
        );
        write_raw_handoff(&run, "plan", &filled);

        let err = phase_done(&run, "plan").unwrap_err().to_string();
        assert!(err.contains("State"), "names a section still at TODO: {err}");
        assert!(
            !err.contains("in Objective") && !err.contains(", Objective"),
            "and not the one that was written: {err}"
        );
    }

    /// A fully written handoff completes.
    #[test]
    fn a_written_handoff_completes() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = registered_phase_run("handoff-written");
        let written = crate::brief::handoff_scaffold().replace(
            crate::brief::SCAFFOLD_PLACEHOLDER,
            "Real content for this section.",
        );
        write_raw_handoff(&run, "plan", &written);

        phase_done(&run, "plan").expect("a filled scaffold completes");
    }

    /// A hand-written handoff that never went through the scaffold is untouched by the gate.
    #[test]
    fn a_hand_written_handoff_still_completes() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = registered_phase_run("handoff-hand-written");
        write_raw_handoff(
            &run,
            "plan",
            "Ported the parser to the new lexer; interfaces unchanged. Next agent: run the \
             conformance suite before touching codegen.\n",
        );

        phase_done(&run, "plan").expect("prose without the scaffold is a handoff too");
    }

    /// The accepted false positive, pinned so it is a decision rather than a surprise: a
    /// handoff QUOTING a bare `TODO` line is refused. Escapable — the message names the
    /// section, and any edit to that line (indent it, add text) clears it — and preferred
    /// over the silent bypasses that chasing this case produced across five review rounds.
    #[test]
    fn a_quoted_bare_todo_is_refused_and_that_is_the_trade() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = registered_phase_run("handoff-quoted-todo");
        write_raw_handoff(
            &run,
            "plan",
            "## Objective\n\nShipped it. The upstream stub still reads:\n\n```rust\nTODO\n```\n",
        );

        let err = phase_done(&run, "plan").unwrap_err().to_string();
        assert!(err.contains("Objective"), "names where to look: {err}");
        // Indenting the quoted line is enough to get past it.
        write_raw_handoff(
            &run,
            "plan",
            "## Objective\n\nShipped it. The upstream stub still reads:\n\n```rust\n  TODO\n```\n",
        );
        phase_done(&run, "plan").expect("the refusal is escapable by editing that line");
    }

    /// Review round 1 (nit): an unreadable handoff was reported as "missing or empty",
    /// sending the agent to rewrite a file that is already there.
    #[test]
    fn an_unreadable_handoff_is_not_reported_as_missing() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = registered_phase_run("handoff-unreadable");
        // A directory at the handoff path: present, but not readable as a file.
        let hp = run_dir(&run.name).join("plan-HANDOFF.md");
        std::fs::create_dir_all(&hp).unwrap();

        let err = phase_done(&run, "plan").unwrap_err().to_string();
        assert!(
            err.contains("could not be read"),
            "must not claim it is missing: {err}"
        );
    }


    #[test]
    fn phase_done_refuses_to_stamp_a_token_onto_an_untokened_phase() {
        // The write-side mirror of `legacy_phase_completes_only_on_an_untokenized_
        // marker`. That rule is enforced on READ; without the same check on WRITE,
        // `phase done` exits 0 having created disk state `phase_wait` will never
        // accept, and the driver waits out a full timeout on a phase whose agent
        // was told it finished.
        //
        // This is the live mixed-era case, not a hypothetical: the installed drovr
        // mints no tokens (every phase records `pass: None`) while a rebuilt binary
        // in the same shell exports $DROVR_PASS — so an agent can hold a token its
        // phase does not have.
        let _lock = ENV_LOCK.lock().unwrap();
        let mut run = make_run("untokened-write-test");
        run.phases.push(
            {
                let mut p = Phase::new("plan");
                p.status = PhaseStatus::Running;
                p
            }
            .with_pane("p1"),
        );
        assert!(run.phases[0].pass.is_none());
        run.save().unwrap();
        write_handoff(&run, "plan");

        unsafe {
            std::env::set_var(PASS_ENV, "token-from-a-newer-binary");
        }
        let err = phase_done(&run, "plan").unwrap_err();
        unsafe {
            std::env::remove_var(PASS_ENV);
        }
        assert!(
            err.to_string().contains("env -u DROVR_PASS"),
            "the refusal must name the way out — dropping the token, not supplying one: {err}"
        );
        assert!(
            !done_marker(&run.name, "plan").exists(),
            "a marker phase_wait can never accept must not be written at all"
        );

        // ...and the token-less agent this phase really belongs to still completes.
        let marker = phase_done(&run, "plan").unwrap();
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "");
    }

    #[test]
    fn phase_start_surfaces_a_stale_marker_it_cannot_remove() {
        // The whole stale-marker fix rests on this delete succeeding. If it fails
        // for any reason other than "already gone", phase_start must RAISE — a
        // silent `let _ =` would launch the agent into a phase whose next
        // `phase wait` still short-circuits on the old marker.
        use std::os::unix::fs::PermissionsExt;
        /// Restore the directory's permissions even if the test panics. Without
        /// this a panic leaves the run dir at 0o555 with a marker inside, which
        /// `make_run`'s `remove_dir_all` cannot clear — so EVERY later run of this
        /// test fails, and its panic (under the held ENV_LOCK) poisons the mutex
        /// and reds the whole suite.
        struct RestorePerms(PathBuf, std::fs::Permissions);
        impl Drop for RestorePerms {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, self.1.clone());
            }
        }

        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("unremovable-marker-test");

        let dir = run_dir(&run.name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(done_marker(&run.name, "plan"), b"").unwrap();
        let orig = std::fs::metadata(&dir).unwrap().permissions();
        let _restore = RestorePerms(dir.clone(), orig);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        // Running as root ignores directory permissions — skip rather than assert
        // something the environment cannot produce.
        let root = std::fs::write(dir.join(".probe"), b"").is_ok();
        let res = phase_start(&h, &mut run, "plan", None);

        if root {
            return;
        }
        let err = res.expect_err("an unremovable stale marker must fail phase_start");
        assert!(
            err.to_string().contains("stale completion marker"),
            "the error must name what went wrong: {err}"
        );
        assert!(
            !h.calls().iter().any(|c| c.contains("pane_run")),
            "the agent must not be launched when the marker could not be cleared: {:?}",
            h.calls()
        );
    }

    #[test]
    fn phase_start_rejects_an_empty_phase_name() {
        // `Phase::default()` is representable with `name: ""`; the only site that
        // appends a phase must refuse to create one, so an unnamed phase is never
        // addressable via find_phase.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("empty-name-test");
        assert!(phase_start(&h, &mut run, "", None).is_err());
        assert!(phase_start(&h, &mut run, "   ", None).is_err());
        for bad in ["../../etc/x", "a/b", "a\\b", ".hidden", "a\0b", ".."] {
            assert!(
                phase_start(&h, &mut run, bad, None).is_err(),
                "phase name {bad:?} must be rejected: it lands the marker and the \
                 handoff outside the run dir"
            );
        }
        assert!(run.phases.is_empty(), "no phase may be appended");

        // `Phase::default()` keeps `name: ""` representable (the
        // `..Default::default()` pattern needs `Default`), so instead every entry
        // point refuses to ADDRESS one. Even a hand-edited state.json holding an
        // unnamed phase is inert: no command can reach it.
        let mut unnamed = Phase::new("");
        unnamed.status = PhaseStatus::Running;
        unnamed.set_pane("p1");
        run.phases.push(unnamed);
        assert_eq!(run.phases[0].name, "");
        assert!(phase_done(&run, "").is_err(), "phase_done must refuse");
        assert!(
            phase_wait(&h, &mut run, "", 10).is_err(),
            "phase_wait must refuse"
        );
        assert!(
            phase_send(&h, &mut run, "", "text").is_err(),
            "phase_send must refuse"
        );
        assert!(
            spawn_reviewer(
                &h,
                &mut run,
                "",
                None,
                &AgentLaunch::for_test("claude", "claude")
            )
            .is_err(),
            "spawn_reviewer must refuse an unnamed reviewer too"
        );
        assert!(run.review_phases.is_empty());
    }

    #[test]
    fn launch_exports_the_pass_token_to_the_agent() {
        // The token has to reach the agent's ENVIRONMENT (not just state.json):
        // that is what makes it immutable for the life of that agent, and so what
        // makes a marker attributable to the pass that wrote it.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("pass-env-test");
        phase_start(&h, &mut run, "plan", None).unwrap();

        let pass = run.phases[0].pass.clone().unwrap();
        let calls = h.calls();
        let run_call = calls.iter().find(|c| c.contains("pane_run")).unwrap();
        assert!(
            run_call.contains(&format!("DROVR_PASS='{pass}'")),
            "pane_run must export the pass token, single-quoted: {run_call}"
        );
    }

    #[test]
    fn collect_reads_handoff_file() {
        let _lock = ENV_LOCK.lock().unwrap();
        let mut run = make_run("collect-reads-test");
        let dir = run_dir(&run.name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("brainstorm-HANDOFF.md"), "## handoff content").unwrap();
        run.phases = vec![]; // phases not relevant for collect

        let content = collect(&run, "brainstorm").unwrap();
        assert_eq!(content, "## handoff content");
    }

    #[test]
    fn collect_rejects_a_path_escaping_phase_name() {
        // `collect` interpolates the name into `<run_dir>/<phase>-HANDOFF.md`, so
        // it needs the same guard as every other name-to-path entry point.
        let _lock = ENV_LOCK.lock().unwrap();
        let run = make_run("collect-traversal-test");
        for bad in ["../../../../etc/passwd", "a/b", ".hidden", ""] {
            let err = collect(&run, bad).unwrap_err();
            // Must be the NAME guard, not an incidental "file not found" — the
            // latter would pass even with no guard at all, while still having
            // constructed (and stat'd) a path outside the run dir.
            assert_eq!(
                err.kind(),
                io::ErrorKind::InvalidInput,
                "collect must refuse {bad:?} by name, not by failing to read it: {err}"
            );
            assert!(err.to_string().contains("invalid phase name"), "{err}");
        }
    }

    #[test]
    fn collect_missing_file_returns_err() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = make_run("collect-missing-test");
        let result = collect(&run, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn phase_start_uses_project_dir_as_cwd() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("proj-cwd-test");
        run.project_dir = "/home/user/my-project".into();

        // EVERY phase's tab must carry project_dir as its cwd — the first one
        // included, now that it no longer inherits the root pane's cwd (which
        // was set at workspace create).
        phase_start(&h, &mut run, "brainstorm", None).unwrap();
        phase_start(&h, &mut run, "plan", None).unwrap();

        let calls = h.calls();
        let tabs: Vec<&String> = calls.iter().filter(|c| c.contains("tab_create")).collect();
        assert_eq!(tabs.len(), 2, "one tab per phase: {calls:?}");
        for tab_call in tabs {
            assert!(
                tab_call.contains("cwd=/home/user/my-project"),
                "tab_create must use project_dir as cwd, got: {tab_call}"
            );
        }
    }

    // -- A1: phase_start tags the launch with DROVR_PHASE=<run>/<phase> --------
    #[test]
    fn phase_start_sets_drovr_phase() {
        let _lock = ENV_LOCK.lock().unwrap();
        // Hermetic: clear the config-dir/secret env so the command shape is
        // deterministic regardless of the ambient environment this test runs in.
        // (CLAUDE_CONFIG_DIR, when set, IS inlined — see the dedicated test below.)
        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
            std::env::remove_var("ANTHROPIC_API_KEY");
        }
        let h = FakeHerdr::new();
        let mut run = make_run("start-test");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();

        let calls = h.calls();
        let run_call = calls.iter().find(|c| c.contains("pane_run")).unwrap();
        assert!(
            run_call.contains(r"DROVR_PHASE='start-test/brainstorm' DROVR_PASS="),
            "pane_run command must carry a single-quoted DROVR_PHASE=<run>/<phase>: {run_call}"
        );
        // Auth secrets must never be inlined into the launch command.
        assert!(
            !run_call.contains("ANTHROPIC_API_KEY"),
            "no secret in command: {run_call}"
        );
        assert!(
            !run_call.contains("CLAUDE_CONFIG_DIR"),
            "config dir absent when unset: {run_call}"
        );
    }

    // -- A1b: when CLAUDE_CONFIG_DIR is set, it IS inlined (single-quoted) into
    //    the launch command so the spawned agent authenticates as the caller's
    //    profile. It is a path, not a secret, so inlining is safe (see
    //    `launch_in_pane`). This locks the behavior that made phase_start_sets_
    //    drovr_phase environment-sensitive before it was made hermetic.
    #[test]
    fn phase_start_inlines_claude_config_dir_when_set() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("CLAUDE_CONFIG_DIR", "/home/user/.config/claude-work");
            std::env::remove_var("ANTHROPIC_API_KEY");
        }
        let h = FakeHerdr::new();
        let mut run = make_run("cfg-dir-test");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();
        // Restore the env immediately (before any fallible assertion) so no code
        // path out of this test leaks the var to the next ENV_LOCK holder.
        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
        }

        let calls = h.calls();
        let run_call = calls.iter().find(|c| c.contains("pane_run")).unwrap();
        assert!(
            run_call.contains(
                r"CLAUDE_CONFIG_DIR='/home/user/.config/claude-work' claude"
            ) && run_call.contains(r"DROVR_PHASE='cfg-dir-test/brainstorm'"),
            "CLAUDE_CONFIG_DIR must be inlined single-quoted alongside DROVR_PHASE: {run_call}"
        );
        // A real secret still never rides the command line.
        assert!(
            !run_call.contains("ANTHROPIC_API_KEY"),
            "no secret in command: {run_call}"
        );
    }

    // -- F1 (agy security): originally `phase_start_shell_quotes_unsafe_phase_name`
    //    — a phase name with shell metacharacters had to survive as one quoted
    //    word. It is now REJECTED at the boundary instead (see
    //    `require_phase_name`), so the quoting proof moved to
    //    `launch_in_pane_quotes_every_value_it_interpolates`, which uses a RUN
    //    name — still unrestricted — to exercise the same code path. See
    //    `phase_start_rejects_a_shell_metacharacter_phase_name` for the boundary.
    //    `shell_single_quote` itself is unit-tested in `crate::shell`.

    // -- F2 (agy correctness), rewritten for the root-pane decoupling: a failed
    //    launch must leave the root shell exactly as it found it. It no longer
    //    "keeps" the root pane by deferring a consumption — nothing consumes it
    //    at all — so what this pins now is that the failing launch went to the
    //    phase's own tab and the anchor is still recorded.
    #[test]
    fn a_failed_launch_leaves_the_root_pane_untouched() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        h.fail_pane_run();
        let mut run = make_run_with_workspace("launch-fail-test", "ws-9");

        let res = phase_start(&h, &mut run, "brainstorm", None);
        assert!(
            res.is_err(),
            "phase_start must propagate the pane_run failure"
        );
        assert_eq!(
            run.root_pane.as_deref(),
            Some("ws-9:root"),
            "the root shell is never consumed, failure or not"
        );
        let calls = h.calls();
        assert!(
            !calls.iter().any(|c| c.contains("pane_run pane=ws-9:root")),
            "the launch must have targeted the phase's own tab: {calls:?}"
        );

        // The tab this call created must not be abandoned. Every phase now
        // creates one, so a failed launch is the common path to an ORPHAN: a
        // pane drovr opened and never recorded. `drovr cleanup` closes only
        // panes it can prove are drovr's, so an unrecorded one is treated as
        // the human's — left forever, AND blocking `workspace_close` for the
        // whole run. Recording it is what keeps cleanup able to reclaim it.
        let orphan = calls
            .iter()
            .find(|c| c.contains("tab_create"))
            .and_then(|c| c.rsplit("-> ").next())
            .expect("the failing phase created a tab")
            .to_owned();
        assert!(
            run.retired_panes.contains(&orphan),
            "a pane whose launch failed must be retired so cleanup still owns it: \
             retired={:?} orphan={orphan}",
            run.retired_panes
        );
        assert!(
            calls
                .iter()
                .any(|c| c.contains(&format!("pane_close pane={orphan}"))),
            "and closed best-effort, so it does not sit there dead: {calls:?}"
        );
        // Belt and braces: recording must survive the process, not just this
        // `RunState`, or a retry loses it.
        let reloaded = RunState::load("launch-fail-test").expect("state must be on disk");
        assert!(
            reloaded.retired_panes.contains(&orphan),
            "the retirement must be PERSISTED before the error propagates: {:?}",
            reloaded.retired_panes
        );
    }

    /// A reviewer's tab is orphaned by a failed launch exactly like a phase's.
    ///
    /// This one is NOT fallout from the root-pane change — `spawn_reviewer` has
    /// always created its own tab — but it is the same class, and it strands a
    /// workspace the same way: `review_phases` registration happens only after
    /// the launch succeeds, so a failure leaves a pane nothing records, which
    /// `drovr cleanup` then protects as the human's. Fixing one instance of a
    /// class and leaving the other is how the class survives.
    #[test]
    fn a_reviewer_whose_launch_fails_does_not_strand_its_tab() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        h.fail_pane_run();
        let mut run = make_run_with_workspace("rev-orphan", "ws-ro");

        let res = spawn_reviewer(
            &h,
            &mut run,
            "review:task-1:1:correctness",
            None,
            &AgentLaunch::for_test("claude", "claude --permission-mode plan"),
        );
        assert!(res.is_err(), "the pane_run failure must propagate");
        assert!(
            run.review_phases.is_empty(),
            "a reviewer that never launched must not be registered"
        );

        let calls = h.calls();
        let orphan = calls
            .iter()
            .find(|c| c.contains("tab_create"))
            .and_then(|c| c.rsplit("-> ").next())
            .expect("the reviewer created a tab")
            .to_owned();
        assert!(
            run.retired_panes.contains(&orphan),
            "the orphan must be recorded so cleanup still owns it: {:?}",
            run.retired_panes
        );
        assert!(
            calls
                .iter()
                .any(|c| c.contains(&format!("pane_close pane={orphan}"))),
            "and closed best-effort: {calls:?}"
        );
    }

    /// A close that fails must still leave the pane recorded — that is the whole
    /// reason the record comes first.
    #[test]
    fn an_orphan_tab_stays_recorded_when_it_cannot_be_closed() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        h.fail_pane_run();
        h.fail_pane_close();
        let mut run = make_run_with_workspace("launch-fail-noclose", "ws-nc");

        assert!(phase_start(&h, &mut run, "brainstorm", None).is_err());

        assert_eq!(
            run.retired_panes.len(),
            1,
            "the orphan is recorded even though the close failed: {:?}",
            run.retired_panes
        );
    }

    // -- Task B: liveness net -------------------------------------------------

    #[test]
    fn detect_stuck_prompt_matches_renderer_upsell() {
        let pane = "some output\nTry the new fullscreen renderer?\n❯ 1. Yes\n  2. Not now";
        assert_eq!(detect_stuck_prompt(pane), Some("try the new fullscreen"));
    }

    #[test]
    fn detect_stuck_prompt_matches_trust_dialog() {
        let pane = "Do you trust the files in this folder?\n❯ 1. Yes, proceed";
        // The first signature scanned that matches wins; both are present here.
        assert!(detect_stuck_prompt(pane).is_some());
    }

    #[test]
    fn detect_stuck_prompt_is_case_insensitive() {
        assert!(detect_stuck_prompt("TRY THE NEW FULLSCREEN RENDERER?").is_some());
        assert!(detect_stuck_prompt("Enter To Confirm").is_some());
    }

    #[test]
    fn detect_stuck_prompt_none_on_normal_output() {
        // Ordinary agent working output must NOT be flagged as a stuck prompt.
        let pane = "Reading files...\nEditing src/main.rs\nRunning cargo test\nAll tests pass.";
        assert_eq!(detect_stuck_prompt(pane), None);
    }

    #[test]
    fn tail_snippet_returns_last_nonblank_lines() {
        let pane = "a\n\nb\n\n\nc\nd\n\n";
        assert_eq!(tail_snippet(pane, 2), "c\nd");
        // Fewer lines than requested returns all of them.
        assert_eq!(tail_snippet("only\n", 6), "only");
    }

    #[test]
    fn diagnose_stuck_phase_surfaces_actionable_diagnostic() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("stuck-test");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();
        // The pane read on the next timeout returns a parked prompt.
        h.push_read("Try the new fullscreen renderer?\n❯ 1. Yes\n  2. Not now");

        let diag = diagnose_stuck_phase(&h, &run, "brainstorm")
            .expect("a parked prompt must yield a diagnostic");
        // Names the run + phase, quotes the pane, and points at `drovr attach`.
        assert!(
            diag.contains("stuck-test"),
            "diag must name the run: {diag}"
        );
        assert!(
            diag.contains("brainstorm"),
            "diag must name the phase: {diag}"
        );
        assert!(
            diag.contains("Try the new fullscreen"),
            "diag must quote the pane: {diag}"
        );
        assert!(
            diag.contains("drovr attach 'stuck-test'"),
            "diag must suggest attach: {diag}"
        );
    }

    #[test]
    fn diagnose_stuck_phase_none_when_working() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("working-test");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();
        // The pane shows ordinary working output — not a stuck prompt.
        h.push_read("Editing src/main.rs\nRunning cargo test\nAll tests pass.");

        assert!(
            diagnose_stuck_phase(&h, &run, "brainstorm").is_none(),
            "working output must not be reported as stuck"
        );
    }

    #[test]
    fn diagnose_stuck_phase_none_without_pane_id() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let run = make_run("no-pane-test"); // no phase started → no pane_id
        // No phase registered at all: nothing to diagnose, no pane read.
        assert!(diagnose_stuck_phase(&h, &run, "brainstorm").is_none());
        assert!(
            !h.calls().iter().any(|c| c.contains("agent_read")),
            "must not read a pane that does not exist yet"
        );
    }

    // -- Task 4: spawn_reviewer --------------------------------------------------

    #[test]
    fn spawn_reviewer_registers_in_review_phases() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("spawn-rev-test", "ws-rv");

        spawn_reviewer(
            &h,
            &mut run,
            "review:task-1:1:correctness",
            None,
            &AgentLaunch::for_test("claude", "claude --permission-mode plan"),
        )
        .unwrap();

        // Registered in review_phases, NOT the pipeline `phases` list.
        assert!(
            run.phases.is_empty(),
            "reviewer must not touch pipeline phases"
        );
        assert_eq!(run.review_phases.len(), 1);
        let p = &run.review_phases[0];
        assert_eq!(p.name, "review:task-1:1:correctness");
        assert_eq!(p.status, PhaseStatus::Running);
        assert!(p.pane_id().is_some(), "pane_id must be recorded");

        let calls = h.calls();
        let run_call = calls.iter().find(|c| c.contains("pane_run")).unwrap();
        assert!(
            run_call.contains(r"DROVR_PHASE='spawn-rev-test/review:task-1:1:correctness'"),
            "pane_run must carry a single-quoted DROVR_PHASE=<run>/<phase>: {run_call}"
        );
        assert!(
            run_call.contains("claude --permission-mode plan"),
            "pane_run must launch the full launch_command: {run_call}"
        );
    }

    /// A reviewer name is REUSED when a panel resume respawns an angle in place
    /// — same task, same iteration, same name — so the reviewer being replaced
    /// can have left a `<phase>.done` behind. `code_review_run` already drops
    /// that reviewer's findings file for exactly this reason; the marker is the
    /// other half. Without the sweep the replacement reads as "finished without
    /// delivering" from its very first poll, and is failed before it has been
    /// asked anything.
    /// Reaping made "this phase has no pane" a state a driver reaches by doing
    /// the normal thing in the wrong order — starting the next phase (which
    /// supersedes this one) and only then `phase send`ing back into it. The
    /// refusal has to name the cause and the recovery, or the driver is left
    /// with "phase has no pane_id" and no next move.
    #[test]
    fn sending_to_a_reaped_phase_says_it_was_reaped_and_how_to_get_it_back() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-to-reaped");
        let mut p = Phase::new("brainstorm");
        p.status = PhaseStatus::Done;
        p.set_pane("ws-mk:p1");
        p.mark_reaped();
        run.phases.push(p);
        // A phase that never had a pane keeps the old, generic refusal: nothing
        // was closed, so there is nothing to bring back.
        run.phases.push(Phase::new("plan"));
        run.save().unwrap();

        let err = phase_send(&h, &mut run, "brainstorm", "hello").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("drovr closed it"), "{msg}");
        assert!(msg.contains("drovr phase rehydrate 'send-to-reaped' 'brainstorm'"), "{msg}");

        let other = phase_send(&h, &mut run, "plan", "hello").unwrap_err().to_string();
        assert!(
            !other.contains("rehydrate"),
            "a phase that never held a pane must not be offered a recovery that \
             would refuse it: {other}"
        );
    }

    #[test]
    fn spawn_reviewer_clears_a_marker_left_by_the_reviewer_it_replaces() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("rev-sweep-test", "ws-rs");
        let phase = "review:task-1:1:correctness";
        let marker = done_marker(&run.name, phase);
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, b"old-pass").unwrap();

        spawn_reviewer(
            &h,
            &mut run,
            phase,
            None,
            &AgentLaunch::for_test("claude", "claude --permission-mode plan"),
        )
        .unwrap();

        assert!(
            !marker.exists(),
            "the replaced reviewer's completion marker must not survive its replacement"
        );
    }

    /// The sweep is `?`, not `let _ =`, for the same reason `phase_start`'s is:
    /// the caller depends on the marker being ABSENT afterwards, so a swallowed
    /// failure launches a reviewer that is complete before it starts.
    #[test]
    fn spawn_reviewer_refuses_to_launch_when_it_cannot_clear_a_stale_marker() {
        use std::os::unix::fs::PermissionsExt;
        struct RestorePerms(PathBuf, std::fs::Permissions);
        impl Drop for RestorePerms {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, self.1.clone());
            }
        }

        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("rev-sweep-fail-test", "ws-rsf");
        let phase = "review:task-1:1:correctness";
        let dir = run_dir(&run.name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(done_marker(&run.name, phase), b"").unwrap();
        let orig = std::fs::metadata(&dir).unwrap().permissions();
        let _restore = RestorePerms(dir.clone(), orig);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        // Running as root ignores directory permissions — skip rather than
        // assert something the environment cannot produce.
        let root = std::fs::write(dir.join(".probe"), b"").is_ok();
        let res = spawn_reviewer(
            &h,
            &mut run,
            phase,
            None,
            &AgentLaunch::for_test("claude", "claude --permission-mode plan"),
        );
        if root {
            return;
        }
        let err = res.expect_err("an unremovable stale marker must fail spawn_reviewer");
        assert!(
            err.to_string().contains("stale completion marker"),
            "the error must name what went wrong: {err}"
        );
        assert!(
            !h.calls().iter().any(|c| c.contains("pane_run")),
            "the reviewer must not be launched when the marker could not be cleared: {:?}",
            h.calls()
        );
    }

    /// The panel's wait loop asks this, and it must answer about THIS pass —
    /// a reviewer name is reused across a respawn, so a marker from the
    /// reviewer that was replaced must not finish its replacement.
    #[test]
    fn a_marker_completes_only_the_pass_whose_token_it_carries() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = make_run("marker-pass-test");
        std::fs::create_dir_all(run_dir(&run.name)).unwrap();
        let marker = done_marker(&run.name, "plan");

        let mut phase = Phase::new("plan");
        phase.pass = PassToken::new("pass-2".into());

        assert!(
            !marker_completes_current_pass(&run.name, &phase),
            "no marker at all completes nothing"
        );
        std::fs::write(&marker, b"pass-1\n").unwrap();
        assert!(
            !marker_completes_current_pass(&run.name, &phase),
            "a marker from a previous pass must not complete this one"
        );
        std::fs::write(&marker, b"").unwrap();
        assert!(
            !marker_completes_current_pass(&run.name, &phase),
            "an untokenized marker must not complete a tokened pass"
        );
        std::fs::write(&marker, b"pass-2\n").unwrap();
        assert!(
            marker_completes_current_pass(&run.name, &phase),
            "this pass's own token completes it"
        );
    }

    #[test]
    fn spawn_reviewer_always_creates_tab_never_root_pane() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("rev-tab-test", "ws-rt");
        // root_pane is Some. Nothing may run there — not a pipeline phase
        // (see `first_phase_creates_its_own_tab_…`) and not a reviewer.
        assert!(run.root_pane.is_some());

        spawn_reviewer(
            &h,
            &mut run,
            "review:task-1:1:security",
            None,
            &AgentLaunch::for_test("claude", "claude --permission-mode plan"),
        )
        .unwrap();

        let calls = h.calls();
        let tab_call = calls
            .iter()
            .find(|c| c.contains("tab_create"))
            .expect("reviewer must create its own tab");
        assert!(
            tab_call.contains("workspace=ws-rt"),
            "tab in the run workspace: {tab_call}"
        );
        // Root pane untouched — it anchors the workspace for the whole run.
        assert_eq!(
            run.root_pane.as_deref(),
            Some("ws-rt:root"),
            "reviewer must not consume the workspace root pane"
        );
        let reviewer_pane = run.review_phases[0].pane_id().map(str::to_owned).unwrap();
        assert_ne!(
            reviewer_pane, "ws-rt:root",
            "reviewer must not run in the root pane"
        );
    }

    #[test]
    fn spawn_reviewer_shell_quotes_an_unsafe_run_name() {
        // Was `spawn_reviewer_shell_quotes_unsafe_phase_name`: an unsafe PHASE
        // name is now rejected outright (see
        // `spawn_reviewer_rejects_a_shell_metacharacter_phase_name`), so the
        // quoting half of the pair moved onto the run name — which is checked
        // for path safety only and so still reaches the command with
        // metacharacters intact.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("rev-inject-test; id", "ws-ri");

        spawn_reviewer(
            &h,
            &mut run,
            "review:t:1:correctness",
            None,
            &AgentLaunch::for_test("claude", "claude --permission-mode plan"),
        )
        .unwrap();

        let calls = h.calls();
        let run_call = calls.iter().find(|c| c.contains("pane_run")).unwrap();
        assert!(
            run_call.contains(r"DROVR_PHASE='rev-inject-test; id/review:t:1:correctness'"),
            "an unsafe run name must be single-quoted: {run_call}"
        );
    }

    #[test]
    fn spawn_reviewer_provisions_a_workspace_the_run_never_got() {
        // Was `spawn_reviewer_errors_without_workspace`. A reviewer needs a tab,
        // a tab needs a workspace, and a workspace is now something drovr makes
        // rather than something it refuses over.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("rev-no-ws-test");
        run.workspace = None;

        spawn_reviewer(
            &h,
            &mut run,
            "review:t:1:correctness",
            None,
            &AgentLaunch::for_test("claude", "claude --permission-mode plan"),
        )
        .expect("a reviewer must not be blocked by a missing workspace");

        let ws = run.workspace.clone().expect("a workspace must be recorded");
        assert!(
            h.calls()
                .iter()
                .any(|c| c.contains(&format!("tab_create workspace={ws}"))),
            "the reviewer tab belongs to the workspace just created: {:?}",
            h.calls()
        );
    }

    #[test]
    fn spawn_reviewer_records_seed_and_phase_send_routes_to_it() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("rev-seed-test", "ws-rs");
        let seed = Path::new("/tmp/review-seed.md");

        spawn_reviewer(
            &h,
            &mut run,
            "review:t:1:correctness",
            Some(seed),
            &AgentLaunch::for_test("claude", "claude --permission-mode plan"),
        )
        .unwrap();

        // Seed path recorded on handoff_doc for later injection — NOT on the command line.
        assert_eq!(
            run.review_phases[0].handoff_doc.as_deref(),
            Some("/tmp/review-seed.md")
        );
        let run_call = h
            .calls()
            .into_iter()
            .find(|c| c.contains("pane_run"))
            .unwrap();
        assert!(
            !run_call.contains("/tmp/review-seed.md"),
            "seed must not be on the command line: {run_call}"
        );

        // phase_send routes to the reviewer pane registered in review_phases.
        // Report the pane ready so the readiness gate returns on the first poll.
        h.push_status(Some("idle"));
        phase_send(&h, &mut run, "review:t:1:correctness", "here is your brief").unwrap();
        let send_call = h
            .calls()
            .into_iter()
            .rev()
            .find(|c| c.contains("agent_prompt_confirm"))
            .unwrap();
        let reviewer_pane = run.review_phases[0].pane_id().map(str::to_owned).unwrap();
        assert!(
            send_call.contains(&reviewer_pane),
            "send must route to the reviewer pane: {send_call}"
        );
        assert!(send_call.contains("here is your brief"));
    }

    #[test]
    fn spawn_reviewer_preserves_focus() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("rev-focus-test", "ws-rf");

        spawn_reviewer(
            &h,
            &mut run,
            "review:t:1:correctness",
            None,
            &AgentLaunch::for_test("claude", "claude --permission-mode plan"),
        )
        .unwrap();

        let calls = h.calls();
        let capture = calls
            .iter()
            .position(|c| c.contains("focused_workspace"))
            .unwrap();
        let run_at = calls.iter().position(|c| c.contains("pane_run")).unwrap();
        let restore = calls
            .iter()
            .position(|c| c.contains("workspace_focus"))
            .unwrap();
        assert!(
            capture < run_at,
            "focus must be captured before pane_run: {calls:?}"
        );
        assert!(
            restore > run_at,
            "focus must be restored after pane_run: {calls:?}"
        );
    }

    #[test]
    fn phase_start_empty_project_dir_returns_error() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("empty-proj-dir-test");
        run.project_dir = String::new();

        let result = phase_start(&h, &mut run, "brainstorm", None);
        assert!(result.is_err(), "must error when project_dir is empty");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("project_dir"),
            "error should mention project_dir: {msg}"
        );
        // This guard fires BEFORE `ensure_workspace`, so it is the message a user
        // actually sees for this cause — and it used to be the one telling a run
        // with committed work to start over.
        assert!(
            !msg.contains("recreate the run"),
            "no refusal may answer a repairable run with 'start over': {msg}"
        );
    }

    /// Every refusal for a missing `project_dir` is one sentence, written once.
    /// Pinned because the failure mode here is drift, not logic: three sites used
    /// to say "please recreate the run with `drovr new`", and a fix that reworded
    /// only the site it happened to touch would leave the other two contradicting
    /// it — which is exactly what the first pass of this change did.
    #[test]
    fn every_missing_project_dir_refusal_names_the_field_instead_of_starting_over() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("proj-dir-wording-test");
        run.project_dir = String::new();

        let from_start = phase_start(&h, &mut run, "brainstorm", None).unwrap_err();
        let from_reviewer = spawn_reviewer(
            &h,
            &mut run,
            "review:t:1:correctness",
            None,
            &AgentLaunch::for_test("claude", "claude"),
        )
            .expect_err("a reviewer needs a project_dir too");
        // `ensure_workspace` only needs a project_dir when it has to CREATE
        // something — with a live workspace it never looks — so drop the
        // workspace to reach its refusal.
        run.workspace = None;
        let from_ensure =
            ensure_workspace(&h, &mut run).expect_err("no workspace and no cwd to open one in");
        let canonical = missing_project_dir_error("proj-dir-wording-test").to_string();

        for (site, msg) in [
            ("phase_start", from_start.to_string()),
            ("spawn_reviewer", from_reviewer.to_string()),
            ("ensure_workspace", from_ensure.to_string()),
        ] {
            assert_eq!(msg, canonical, "{site} must raise the shared refusal");
        }
        // And the shared refusal says where to fix it, not to abandon the run.
        assert!(canonical.contains("state.json"), "{canonical}");
        assert!(!canonical.contains("recreate the run"), "{canonical}");
    }

    #[test]
    fn spawn_reviewer_empty_project_dir_returns_error() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("rev-empty-proj-test", "ws-e");
        run.project_dir = String::new();

        let result = spawn_reviewer(
            &h,
            &mut run,
            "review:t:1:correctness",
            None,
            &AgentLaunch::for_test("claude", "claude"),
        );
        assert!(
            result.is_err(),
            "reviewer must error when project_dir is empty"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("project_dir"),
            "error should mention project_dir: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // Shell-injection surface: a phase name reaches drovr from argv AND from the
    // review server's HTTP layer (`review:<task>:…`, where `<task>` is only
    // checked for path safety), and drovr PRINTS copy-pasteable remediation
    // commands with run/phase/token values in them. The delivery mechanism is a
    // human pasting drovr's own suggestion into a shell. Two independent rules:
    // the validation boundary rejects a name that could not be a phase name, and
    // every emission site quotes whatever it interpolates.
    // -----------------------------------------------------------------------

    #[test]
    fn require_new_phase_name_rejects_shell_metacharacters() {
        for bad in [
            "p; rm -rf ~",
            "p && id",
            "p | tee /tmp/x",
            "p`id`",
            "p$(id)",
            "p$HOME",
            "p > out",
            "p<in",
            "p&",
            "p\nid",
            "p 'q'",
            "p\"q\"",
            "p*",
            "p?",
            "p~",
            "p!",
            "p#c",
            "p{a,b}",
            "p[a]",
            "p(a)",
            "p q",
            "p\ta",
        ] {
            assert!(
                require_new_phase_name(bad).is_err(),
                "a phase name a shell would not read as one literal word must not \
                 be CREATED: {bad:?}"
            );
        }
    }

    #[test]
    fn require_new_phase_name_accepts_the_names_drovr_actually_uses() {
        for ok in [
            "brainstorm",
            "implement-task-1-fixes-2",
            "review:task-1:1:correctness",
            "review:task-1:12:type-design",
            "a_b",
            "v1.2",
            "PHASE9",
        ] {
            assert!(
                require_new_phase_name(ok).is_ok(),
                "the hardening must not reject a name drovr itself mints: {ok:?}"
            );
        }
    }

    #[test]
    fn the_resolve_rule_stays_weaker_than_the_creation_rule() {
        // The asymmetry is load-bearing, not an oversight — see
        // `a_phase_already_on_disk_under_an_old_name_is_still_reachable`. Both
        // still reject what would escape the run dir.
        assert!(require_new_phase_name("a b").is_err());
        assert!(require_phase_name("a b").is_ok());
        for escaping in ["../x", "a/b", "a\\b", "..", ".hidden", "", "  "] {
            assert!(require_phase_name(escaping).is_err(), "{escaping:?}");
            assert!(require_new_phase_name(escaping).is_err(), "{escaping:?}");
        }
    }

    #[test]
    fn phase_start_rejects_a_shell_metacharacter_phase_name() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("inject-reject-test");

        let err = phase_start(&h, &mut run, "p; rm -rf ~", None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(
            !h.calls().iter().any(|c| c.contains("pane_run")),
            "a rejected name must never reach a launch: {:?}",
            h.calls()
        );
    }

    #[test]
    fn spawn_reviewer_rejects_a_shell_metacharacter_phase_name() {
        // The HTTP-reachable half: `review:<task>:<iter>:<angle>` is built from a
        // task string the review server only checks for path safety.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("inject-reviewer-test", "ws-i");

        let err = spawn_reviewer(
            &h,
            &mut run,
            "review:t$(id):1:correctness",
            None,
            &AgentLaunch::for_test("claude", "claude"),
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(
            !h.calls().iter().any(|c| c.contains("pane_run")),
            "a rejected reviewer name must never reach a launch: {:?}",
            h.calls()
        );
    }

    #[test]
    fn launch_in_pane_quotes_every_value_it_interpolates() {
        // Defence in depth behind `require_phase_name`: run names and pass tokens
        // are NOT restricted the way phase names are, and this is the one command
        // string drovr hands to a shell rather than to a human.
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
        }
        let h = FakeHerdr::new();

        launch_in_pane(
            &h,
            "r; rm -rf ~",
            "plan",
            "p1",
            "claude",
            &PassToken::new("t'k".into()).unwrap(),
            // A rehydrate passes a RECORDED profile, which came off disk and is
            // no more trustworthy than the run name beside it.
            Some("/pro'file"),
        )
        .unwrap();

        let calls = h.calls();
        let run_call = calls.iter().find(|c| c.contains("pane_run")).unwrap();
        assert!(
            run_call.contains(r"CLAUDE_CONFIG_DIR='/pro'\\''file'"),
            "the profile is quoted the same way: {run_call}"
        );
        assert!(
            run_call.contains(r"DROVR_PHASE='r; rm -rf ~/plan'"),
            "the run name is quoted into one literal word: {run_call}"
        );
        // The fake records the command with `{:?}`, so the backslash of the
        // `'\''` escape appears doubled here; the command itself carries one.
        assert!(
            run_call.contains(r"DROVR_PASS='t'\\''k'"),
            "an embedded quote is escaped, not terminated: {run_call}"
        );
    }

    #[test]
    fn phase_done_remediation_commands_are_quoted() {
        // The refusal prints a command the agent's human is meant to paste. A run
        // name is validated for path safety only, so it can carry metacharacters.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("done-quote-test; id");

        phase_start(&h, &mut run, "plan", None).unwrap();
        write_handoff(&run, "plan");
        let want = run.phases[0].pass.clone().unwrap();

        // Case 1: a tokened phase with no $DROVR_PASS — remedy SUPPLIES the token.
        let err = phase_done(&run, "plan").unwrap_err().to_string();
        assert!(
            err.contains(&format!(
                "DROVR_PASS='{want}' drovr phase done 'done-quote-test; id' 'plan'"
            )),
            "every value in the suggested command must be single-quoted: {err}"
        );

        // Case 2: an untokened phase holding a token — remedy DROPS it.
        let mut legacy = make_run("done-quote-legacy; id");
        legacy.phases.push(
            {
                let mut p = Phase::new("plan");
                p.status = PhaseStatus::Running;
                p
            }
            .with_pane("p1"),
        );
        legacy.save().unwrap();
        write_handoff(&legacy, "plan");
        unsafe {
            std::env::set_var(PASS_ENV, "t-from-elsewhere");
        }
        let err = phase_done(&legacy, "plan").unwrap_err().to_string();
        unsafe {
            std::env::remove_var(PASS_ENV);
        }
        assert!(
            err.contains("env -u DROVR_PASS drovr phase done 'done-quote-legacy; id' 'plan'"),
            "the drop-the-token remedy must be quoted too: {err}"
        );
    }

    #[test]
    fn a_send_that_fails_after_the_re_open_says_the_completion_is_gone() {
        // `reopen_for_re_entry` runs BEFORE `agent_send`, so a send that fails
        // leaves the phase Running with its completion marker already deleted —
        // exactly the state that later reads as a phantom incomplete phase. The
        // bare transport error says nothing about it, and the caller is left
        // believing nothing happened.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-fail-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        write_handoff(&run, "plan");
        let pass = run.phases[0].pass.clone().unwrap();
        agent_signals_done(&run, "plan", &pass);
        run.phases[0].status = PhaseStatus::Done;
        run.save().unwrap();

        h.fail_agent_send();
        let err = phase_send(&h, &mut run, "plan", "next").unwrap_err().to_string();

        assert!(
            !done_marker(&run.name, "plan").exists(),
            "precondition: the re-open really did clear the marker"
        );
        assert!(
            err.contains("completion marker"),
            "the failure must report the state it left behind, not only the \
             transport error: {err}"
        );
        assert!(
            err.contains("plan") && err.contains("send-fail-test"),
            "and name the phase it applies to: {err}"
        );
    }

    #[test]
    fn a_failed_send_to_a_reviewer_phase_claims_no_re_open() {
        // `reopen_for_re_entry` searches `phases` only, so it NO-OPS for a phase
        // that lives in `review_phases` — while `require_pane_id` resolves both,
        // so `phase_send` reaches a reviewer pane happily. A reviewer that has
        // finished and exited is exactly the pane whose `agent_send` fails, and
        // its `.done` marker is intact. Claiming "your marker is deleted and the
        // status is back to Running" there is a false diagnostic about a phase
        // that is genuinely, correctly complete.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-fail-reviewer-test");
        run.review_phases.push(
            {
                let mut p = Phase::new("review:t:1:correctness");
                p.status = PhaseStatus::Done;
                p
            }
            .with_pane("p9"),
        );
        run.save().unwrap();
        // A reviewer's marker, written by the reviewer itself before it exited.
        let marker = done_marker(&run.name, "review:t:1:correctness");
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, b"").unwrap();

        h.fail_agent_send();
        let err = phase_send(&h, &mut run, "review:t:1:correctness", "next")
            .unwrap_err()
            .to_string();

        assert!(
            marker.exists(),
            "precondition: a reviewer phase is not re-opened, so its marker survives"
        );
        assert!(
            !err.contains("completion marker"),
            "the message must not claim a re-open that did not happen: {err}"
        );
        assert!(
            err.contains("review:t:1:correctness"),
            "it must still name the phase and the transport failure: {err}"
        );
    }

    #[test]
    fn a_marker_mismatch_on_an_untokened_phase_says_to_drop_the_token() {
        // The `expected: None` arm. Before `phase_done_command` existed, this
        // path emitted the literal `DROVR_PASS=<none> drovr phase done …` — a
        // command that sets the variable to the string "<none>" and fails the
        // same way again. The remedy for a phase with no token is to DROP the one
        // being held, never to supply a fabricated one.
        let msg = marker_mismatch_message("plan", "r; id", "tok-from-elsewhere", None);
        assert!(
            msg.contains("env -u DROVR_PASS drovr phase done 'r; id' 'plan'"),
            "an untokened phase's remedy drops the token: {msg}"
        );
        assert!(
            !msg.contains("DROVR_PASS=<none>"),
            "never suggest setting the token to a placeholder: {msg}"
        );
        // The evidence half is unchanged: both tokens are still named.
        assert!(msg.contains("tok-from-elsewhere") && msg.contains("<none>"));
    }

    #[test]
    fn a_phase_already_on_disk_under_an_old_name_is_still_reachable() {
        // VERSION SKEW. Before the allowlist, a phase name with a space was legal:
        // an `angles` entry or a `<task>` carrying one produced a real phase, with
        // a pane and a pass token, recorded in state.json. Applying the CREATION
        // rule to every later operation would brick exactly those phases —
        // `phase done`, `phase wait`, `phase send` and `collect` would all refuse
        // the name the live agent was launched under, with no migration path.
        //
        // So the strict alphabet gates CREATION only. The resolve path keeps the
        // path-safety rule, and the emitted commands are quoted (they always must
        // be: run names are unrestricted too).
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("old-name-test");
        let legacy = "review:t:1:api & contracts";
        run.review_phases.push(
            {
                let mut p = Phase::new(legacy);
                p.status = PhaseStatus::Running;
                p
            }
            .with_pane("p7"),
        );
        run.save().unwrap();

        // The name may no longer be CREATED …
        assert!(
            spawn_reviewer(
                &h,
                &mut run,
                legacy,
                None,
                &AgentLaunch::for_test("claude", "claude")
            )
            .is_err(),
            "the creation boundary still rejects it"
        );
        // … but the phase that already exists under it still works end to end.
        let marker = phase_done(&run, legacy).expect("an existing phase can still signal done");
        assert!(marker.exists());
        collect(&run, legacy).expect_err("no handoff file — but the NAME was accepted");
        phase_send(&h, &mut run, legacy, "text").expect("and can still be sent to");
    }

    #[test]
    fn a_name_a_reviewer_already_holds_cannot_become_a_pipeline_phase() {
        // `find_phase_idx` searches `run.phases` only, so a reviewer's name looks
        // brand new to `phase_start` — it appends a SECOND entry under the same
        // name into the other list. `RunState::find_phase` searches `phases`
        // first, so from then on every lookup for that reviewer resolves to the
        // impostor: its pane, its pass token, and the pipeline-only handoff
        // contract. `phase_start` also sweeps `<phase>.done` on the way in,
        // destroying the reviewer's genuine completion marker.
        //
        // A phase name must identify ONE phase.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("name-collision-test", "ws-nc");
        let name = "review:t:1:correctness";
        spawn_reviewer(
            &h,
            &mut run,
            name,
            None,
            &AgentLaunch::for_test("claude", "claude"),
        )
        .unwrap();
        // The reviewer finished and left its evidence.
        let marker = done_marker(&run.name, name);
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, b"").unwrap();

        let err = phase_start(&h, &mut run, name, None).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(
            run.phases.is_empty(),
            "no impostor entry: {:?}",
            run.phases.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
        assert!(
            marker.exists(),
            "the reviewer's completion marker must survive the refusal"
        );
    }

    #[test]
    fn a_name_a_pipeline_phase_already_holds_cannot_become_a_reviewer() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("name-collision-rev-test", "ws-ncr");
        phase_start(&h, &mut run, "plan", None).unwrap();

        let err = spawn_reviewer(
            &h,
            &mut run,
            "plan",
            None,
            &AgentLaunch::for_test("claude", "claude"),
        )
        .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(run.review_phases.is_empty(), "no shadow reviewer");
        assert_eq!(run.phases.len(), 1, "and the pipeline phase is untouched");
    }

    #[test]
    fn a_reviewer_must_be_de_registered_before_it_is_re_spawned() {
        // ⚠️ TASK 6 / MERGE CONTRACT. main's panel resume re-spawns a reviewer
        // under the SAME `review:<task>:<iter>:<angle>` name (it reuses the
        // resumed iter rather than bumping it) — and it drops the stale entry
        // first: `run.review_phases.retain(|p| p.name != phase)`, with the comment
        // "so `find_phase` cannot resolve to the replaced pane". This test states
        // that ordering as a REQUIREMENT rather than a convention: a second entry
        // under a live name is the same corruption as the cross-list case, so
        // `spawn_reviewer` refuses it. Merging main is safe because main already
        // retains-then-spawns; a future respawn that forgets to will fail loudly
        // here instead of silently rerouting the reviewer's pane.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("reviewer-respawn-test", "ws-rr");
        let name = "review:t:1:correctness";

        spawn_reviewer(
            &h,
            &mut run,
            name,
            None,
            &AgentLaunch::for_test("claude", "claude"),
        )
        .unwrap();
        let err = spawn_reviewer(
            &h,
            &mut run,
            name,
            None,
            &AgentLaunch::for_test("claude", "claude"),
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(run.review_phases.len(), 1, "no second entry under one name");

        // Drop the stale registration, exactly as main's respawn does — now it
        // spawns.
        run.review_phases.retain(|p| p.name != name);
        spawn_reviewer(
            &h,
            &mut run,
            name,
            None,
            &AgentLaunch::for_test("claude", "claude"),
        )
        .expect("de-registered first, so the name is free again");
        assert_eq!(run.review_phases.len(), 1);
    }

    #[test]
    fn an_old_named_phase_can_still_be_re_entered() {
        // `phase_start` is BOTH the creation path and the documented re-entry
        // path — `token_lost_message` prints `drovr phase start <run> <phase>` as
        // the recovery for a phase whose token vanished. Gating the whole function
        // on the creation alphabet would break that recovery for exactly the
        // legacy-named phases the create/resolve split exists to keep working.
        // The strict rule applies to a name being INTRODUCED, not to one already
        // in state.json.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("old-name-reentry-test");
        let legacy = "my task 1";
        run.phases.push(
            {
                let mut p = Phase::new(legacy);
                p.status = PhaseStatus::Running;
                p
            }
            .with_pane("p3"),
        );
        run.save().unwrap();

        phase_start(&h, &mut run, legacy, None).expect("re-entry of an existing phase is allowed");
        assert_eq!(
            run.phases.len(),
            1,
            "re-entry reuses the entry, never appends"
        );
        assert!(
            run.phases[0].pass.is_some(),
            "and it really did mint a new pass"
        );

        // A name that does NOT already exist is still refused.
        let err = phase_start(&h, &mut run, "brand new", None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(run.phases.len(), 1, "and nothing was appended");
    }

    #[test]
    fn token_lost_message_quotes_its_suggested_command() {
        let msg = token_lost_message("plan", "r; rm -rf ~");
        assert!(
            msg.contains("drovr phase start 'r; rm -rf ~' 'plan'"),
            "the recovery command must be pasteable, not executable-by-accident: {msg}"
        );
    }
}

// ---------------------------------------------------------------------------
// Session capture — what the poll loops record onto a phase
// ---------------------------------------------------------------------------
//
// herdr DROPS `agent_session` the moment the agent process exits (verified live
// against 0.7.5: an exited pane reports no `agent` and no `agent_session` key at
// all, though `tab_id` survives). So the id can only be read while the agent is
// alive, which is why it is captured opportunistically by the loops that were
// already polling rather than at the moment something wants it.

#[cfg(test)]
mod capture_tests {
    use super::*;
    use crate::config::AgentLaunch;
    use crate::herdr::{AgentSession, FakeHerdr, PaneInfo, SessionId};
    use crate::test_util::ENV_LOCK;

    fn capture_run(name: &str) -> RunState {
        // Caller must hold ENV_LOCK. Mirrors `phase::tests::make_run`.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", format!("/tmp/drovr-capture-test-{name}"));
            std::env::remove_var(PASS_ENV);
            std::env::remove_var("CLAUDE_CONFIG_DIR");
        }
        let _ = std::fs::remove_dir_all(run_dir(name));
        RunState {
            name: name.to_owned(),
            task: "test task".into(),
            agent: Some("claude".into()),
            phases: vec![],
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: Some("ws-cap".into()),
            root_pane: Some("ws-cap:root".into()),
            project_dir: "/tmp/drovr-proj-test".into(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
        }
    }

    fn attached(pane: &str, status: AgentStatus) -> Option<PaneInfo> {
        Some(PaneInfo {
            tab_id: FakeHerdr::tab_id_for(pane),
            agent_status: Some(status),
            agent_session: Some(FakeHerdr::session_for(pane)),
        })
    }

    fn session_less(pane: &str, status: AgentStatus) -> Option<PaneInfo> {
        Some(PaneInfo {
            tab_id: FakeHerdr::tab_id_for(pane),
            agent_status: Some(status),
            agent_session: None,
        })
    }

    /// Drive the readiness gate once, fast. The pane is derived from the phase
    /// now, so this takes no pane argument.
    fn quick(h: &FakeHerdr, run: &mut RunState, phase: &str) -> bool {
        wait_agent_ready(
            h,
            run,
            phase,
            Duration::from_millis(500),
            Duration::from_millis(1),
        )
    }

    #[test]
    fn a_session_survives_herdr_forgetting_it() {
        // THE reason this task exists. herdr reports the session only while the
        // agent is alive; a reaper reading at teardown time would get nothing.
        // So: once captured, a later poll that reports no session — an exited
        // agent — and a poll that fails outright must both LEAVE IT ALONE.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("session-survives");
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pane = run.phases[0].pane_id().map(str::to_owned).unwrap();

        // 1. an attached agent (blocked → not "started", so the gate polls on)
        h.push_pane_info(attached(&pane, AgentStatus::Blocked));
        // 2. the poll FAILS entirely — says nothing about the agent
        h.push_pane_info(None);
        // 3. herdr answers, and the agent has EXITED: no session key at all
        h.push_pane_info(session_less(&pane, AgentStatus::Unknown));
        // 4. ready, still session-less, so the gate returns
        h.push_pane_info(session_less(&pane, AgentStatus::Idle));

        assert!(quick(&h, &mut run, "plan"));

        let want = SessionId::new(FakeHerdr::session_value_for(&pane)).unwrap();
        assert_eq!(
            run.phases[0].pane_agent().and_then(|a| a.session()),
            Some(&want),
            "the last non-empty session must survive both None cases"
        );
        assert_eq!(
            run.phases[0].tab_id.as_deref(),
            Some(FakeHerdr::tab_id_for(&pane).as_str())
        );
        // And it is on DISK, which is the only place a later process can read it.
        let on_disk = RunState::load("session-survives").unwrap();
        assert_eq!(
            on_disk.phases[0].pane_agent().and_then(|a| a.session()),
            Some(&want)
        );
    }

    #[test]
    fn a_session_no_resume_could_use_is_never_recorded() {
        // `kind:"path"` is a transcript path, not an id; an id herdr attributes
        // to another backend would resume the wrong agent's conversation; an
        // unattributed id cannot be checked at all. None of the three may reach
        // `state.json`, because everything downstream of it treats what it finds
        // there as resumable.
        let _lock = ENV_LOCK.lock().unwrap();
        let cases: [(&str, AgentSession); 3] = [
            (
                "path",
                AgentSession::Path {
                    value: "/tmp/transcript.jsonl".into(),
                },
            ),
            (
                "wrong-agent",
                FakeHerdr::session_owned_by("p", Some("cursor")),
            ),
            ("unattributed", FakeHerdr::session_owned_by("p", None)),
        ];
        for (label, session) in cases {
            let h = FakeHerdr::new();
            let mut run = capture_run(&format!("unusable-{label}"));
            phase_start(&h, &mut run, "plan", None).unwrap();
            let pane = run.phases[0].pane_id().map(str::to_owned).unwrap();

            h.push_pane_info(Some(PaneInfo {
                tab_id: FakeHerdr::tab_id_for(&pane),
                agent_status: Some(AgentStatus::Idle),
                agent_session: Some(session),
            }));
            assert!(quick(&h, &mut run, "plan"));

            assert!(
                run.phases[0]
                    .pane_agent()
                    .and_then(|a| a.session())
                    .is_none(),
                "a {label} session must not be captured"
            );
            // The tab id is still recorded — it is a separate, unconditional fact.
            assert_eq!(
                run.phases[0].tab_id.as_deref(),
                Some(FakeHerdr::tab_id_for(&pane).as_str()),
                "{label}: the tab is still readable"
            );
        }
    }

    #[test]
    fn capture_rewrites_state_json_only_when_something_changed() {
        // These loops poll twice a second for the length of a phase — an hour is
        // 7200 saves. Worse than the I/O: every save is a whole-file write, so a
        // no-op one is a chance to clobber a concurrent writer for nothing.
        use std::os::unix::fs::MetadataExt;
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("save-guard");
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pane = run.phases[0].pane_id().map(str::to_owned).unwrap();
        let state = run_dir("save-guard").join("state.json");
        let ino = || std::fs::metadata(&state).unwrap().ino();

        let before = ino();
        // First poll: a session and a tab appear → this MUST be written.
        h.push_pane_info(attached(&pane, AgentStatus::Idle));
        assert!(quick(&h, &mut run, "plan"));
        let after_first = ino();
        assert_ne!(before, after_first, "a new capture must be persisted");
        assert!(
            run.phases[0]
                .pane_agent()
                .and_then(|a| a.session())
                .is_some()
        );

        // Every later poll reports exactly the same thing.
        for _ in 0..3 {
            h.push_pane_info(attached(&pane, AgentStatus::Idle));
            assert!(quick(&h, &mut run, "plan"));
        }
        assert_eq!(
            ino(),
            after_first,
            "an unchanged capture must not rewrite state.json"
        );
    }

    #[test]
    fn capture_does_not_write_the_pollers_stale_snapshot_back() {
        // A `phase wait` holds its `RunState` for as long as the phase runs, and
        // capture happens inside that loop. Persisting through the poller's copy
        // would write an hour-old whole-file snapshot over whatever else the run
        // did meanwhile — the state-clobbering race the wait outcomes were
        // reworked to avoid. So the write goes through freshly loaded state.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("no-clobber");
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pane = run.phases[0].pane_id().map(str::to_owned).unwrap();

        // Another process advances the run while this poller holds its snapshot.
        let mut other = RunState::load("no-clobber").unwrap();
        other.cursor = 7;
        other.gate = "moved-on".into();
        other.save().unwrap();
        assert_eq!(
            run.cursor, 0,
            "the poller's snapshot is stale by construction"
        );

        h.push_pane_info(attached(&pane, AgentStatus::Idle));
        assert!(quick(&h, &mut run, "plan"));

        let on_disk = RunState::load("no-clobber").unwrap();
        assert_eq!(on_disk.cursor, 7, "the other process's work must survive");
        assert_eq!(on_disk.gate, "moved-on");
        assert!(
            on_disk.phases[0]
                .pane_agent()
                .and_then(|a| a.session())
                .is_some(),
            "and the capture must still land"
        );
    }

    #[test]
    fn a_session_appearing_after_the_tab_is_already_known_is_still_captured() {
        // Isolates the cheap guard's session arm. Every other capture test
        // happens to change the TAB on its first poll (it starts unrecorded), so
        // the guard passes on the tab alone and a false negative in the session
        // arm goes unnoticed — verified: inverting that arm passes the whole
        // suite without this test.
        //
        // The real sequence it models is the ordinary one: herdr publishes
        // `agent_status` (and the tab) before `agent_session`, so the first poll
        // that sees the pane commonly records only the tab, and the session
        // arrives on a later poll with everything else identical. If the guard
        // says "nothing new" there, the session is never captured at all.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("session-after-tab");
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pane = run.phases[0].pane_id().map(str::to_owned).unwrap();

        // Poll 1: tab only, no session.
        h.push_pane_info(session_less(&pane, AgentStatus::Idle));
        assert!(quick(&h, &mut run, "plan"));
        assert_eq!(
            run.phases[0].tab_id.as_deref(),
            Some(FakeHerdr::tab_id_for(&pane).as_str()),
            "the tab is recorded on the first poll"
        );
        assert!(
            run.phases[0]
                .pane_agent()
                .and_then(|a| a.session())
                .is_none(),
            "and no session yet — which is the setup, not the assertion"
        );

        // Poll 2: SAME tab, session now published. Nothing but the session differs.
        h.push_pane_info(attached(&pane, AgentStatus::Idle));
        assert!(quick(&h, &mut run, "plan"));

        let want = SessionId::new(FakeHerdr::session_value_for(&pane)).unwrap();
        assert_eq!(
            RunState::load("session-after-tab").unwrap().phases[0]
                .pane_agent()
                .and_then(|a| a.session()),
            Some(&want),
            "a session arriving after the tab must not be filtered out by the guard"
        );
    }

    #[test]
    fn a_capture_failure_is_reported_once_per_phase_not_once_per_poll() {
        // Capture is best-effort and must never fail a wait — but "best-effort"
        // was SILENT, and the failure it hid is not self-healing: if the agent
        // exits before a later poll retries, the session is gone and the only
        // symptom is a rehydrate, much later, that has nothing to resume.
        //
        // It has to be once per phase, though. This sits in a loop that runs
        // twice a second; a diagnostic that repeats at that rate is one nobody
        // reads, and it would bury the wait's real output.
        let seen = std::sync::Mutex::new(std::collections::BTreeSet::new());
        assert!(
            first_capture_failure_for(&seen, "r/plan"),
            "the first says so"
        );
        assert!(
            !first_capture_failure_for(&seen, "r/plan"),
            "every later one is silent"
        );
        assert!(
            first_capture_failure_for(&seen, "r/implement"),
            "a different phase is its own event"
        );
        assert!(
            first_capture_failure_for(&seen, "other/plan"),
            "and so is the same phase name in another run"
        );

        // Bounded: the always-on server never restarts, so the set cannot grow
        // forever. Past the cap it clears, and the cost is one repeated
        // diagnostic — the same order as the guarantee it protects.
        for i in 0..600 {
            first_capture_failure_for(&seen, &format!("r/phase-{i}"));
        }
        assert!(seen.lock().unwrap().len() < 600, "the set must be bounded");
    }

    #[test]
    fn a_capture_whose_save_failed_is_also_retried() {
        // The load-failure path has its own test; this is the other half. They
        // are separate branches with separate diagnostics, and the reason both
        // matter is the same: the caller's copy must NOT adopt a value that
        // never reached disk, or the guard sees "nothing new" on every later
        // poll and the session is lost for the life of the phase.
        use std::os::unix::fs::PermissionsExt;
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("save-fails");
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pane = run.phases[0].pane_id().map(str::to_owned).unwrap();
        let dir = run_dir("save-fails");

        // The run dir is readable but not writable: `RunState::load` still
        // works, so capture gets all the way to the save and fails there.
        let orig = std::fs::metadata(&dir).unwrap().permissions();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        h.push_pane_info(attached(&pane, AgentStatus::Idle));
        let ready = quick(&h, &mut run, "plan");
        // Restore before asserting, so a failure cannot leave an undeletable dir.
        std::fs::set_permissions(&dir, orig).unwrap();

        assert!(ready, "a failed capture write must never fail the wait");
        assert!(
            run.phases[0]
                .pane_agent()
                .and_then(|a| a.session())
                .is_none(),
            "an unpersisted capture must not be claimed in memory either"
        );

        // Writable again: the very next poll reports the same thing and must
        // land it.
        h.push_pane_info(attached(&pane, AgentStatus::Idle));
        assert!(quick(&h, &mut run, "plan"));
        let want = SessionId::new(FakeHerdr::session_value_for(&pane)).unwrap();
        assert_eq!(
            RunState::load("save-fails").unwrap().phases[0]
                .pane_agent()
                .and_then(|a| a.session()),
            Some(&want),
            "the retry must land the session on disk"
        );
    }

    #[test]
    fn a_capture_that_could_not_be_persisted_is_retried() {
        // The subtle half of "best-effort". A poll reports a session; the state
        // read needed to persist it fails. If the caller's own copy were updated
        // anyway, EVERY later poll would report the same session, find that copy
        // already holding it, and conclude there is nothing to do — so one
        // transient failure at the moment a session first appears would lose it
        // for the whole phase. A session id does not change again to re-trigger
        // the guard, and this is the only window in which it can be read at all.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("persist-retry");
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pane = run.phases[0].pane_id().map(str::to_owned).unwrap();
        let state = run_dir("persist-retry").join("state.json");
        let good = std::fs::read(&state).unwrap();

        // `RunState::load` cannot parse this, so the capture cannot be written.
        std::fs::write(&state, b"{ not json").unwrap();
        h.push_pane_info(attached(&pane, AgentStatus::Idle));
        assert!(quick(&h, &mut run, "plan"));
        assert!(
            run.phases[0]
                .pane_agent()
                .and_then(|a| a.session())
                .is_none(),
            "an unpersisted capture must not be claimed in memory either"
        );

        // The very next poll reports exactly the same thing. It must try again.
        std::fs::write(&state, &good).unwrap();
        h.push_pane_info(attached(&pane, AgentStatus::Idle));
        assert!(quick(&h, &mut run, "plan"));

        let want = SessionId::new(FakeHerdr::session_value_for(&pane)).unwrap();
        assert_eq!(
            RunState::load("persist-retry").unwrap().phases[0]
                .pane_agent()
                .and_then(|a| a.session()),
            Some(&want),
            "the retry must land the session on disk"
        );
        assert_eq!(
            run.phases[0].pane_agent().and_then(|a| a.session()),
            Some(&want)
        );
    }

    #[test]
    fn the_callers_copy_adopts_the_launch_record_from_disk_not_a_rebuilt_one() {
        // The caller's `RunState` can be missing a `PhaseAgent` that disk already
        // has — another process launched or re-entered the phase after this one
        // loaded. Capture must seed the caller from DISK, not rebuild a record
        // from what it happens to know: a rebuilt one carries `profile: None`,
        // and the caller's next `run.save()` would write that over the real
        // profile, which is exactly what a resume needs to find the session.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("adopt-from-disk");
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pane = run.phases[0].pane_id().map(str::to_owned).unwrap();

        // Disk gains a full launch record; this caller's copy has none.
        let mut other = RunState::load("adopt-from-disk").unwrap();
        other.phases[0].record_launch("claude", Some("/home/u/.config/claude-work".into()));
        other.save().unwrap();
        run.phases[0].clear_pane_agent_for_test();

        h.push_pane_info(attached(&pane, AgentStatus::Idle));
        assert!(quick(&h, &mut run, "plan"));

        let agent = run.phases[0].pane_agent().expect("seeded from disk");
        assert_eq!(
            agent.profile(),
            Some("/home/u/.config/claude-work"),
            "the profile on disk must survive — a rebuilt record would say None"
        );
        assert!(agent.session().is_some(), "and the capture still lands");

        // And saving the caller's copy must not undo it.
        run.save().unwrap();
        assert_eq!(
            RunState::load("adopt-from-disk").unwrap().phases[0]
                .pane_agent()
                .and_then(|a| a.profile()),
            Some("/home/u/.config/claude-work")
        );
    }

    #[test]
    fn a_capture_already_on_disk_is_adopted_without_a_write() {
        // The inner guard, in the one situation the outer one cannot cover: the
        // caller's copy is behind, so it asks — but the freshly loaded state
        // already carries exactly this capture, because another writer got there
        // first. Writing anyway would be a whole-file save with nothing to say.
        use std::os::unix::fs::MetadataExt;
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("already-on-disk");
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pane = run.phases[0].pane_id().map(str::to_owned).unwrap();
        let state = run_dir("already-on-disk").join("state.json");
        let ino = || std::fs::metadata(&state).unwrap().ino();

        // Another process captured the same session and persisted it.
        let mut other = RunState::load("already-on-disk").unwrap();
        let want = SessionId::new(FakeHerdr::session_value_for(&pane)).unwrap();
        other.phases[0].record_launch("claude", None);
        assert!(other.phases[0].record_session(want.clone()));
        other.phases[0].tab_id = Some(FakeHerdr::tab_id_for(&pane).as_str().to_owned());
        other.save().unwrap();
        assert!(
            run.phases[0]
                .pane_agent()
                .and_then(|a| a.session())
                .is_none(),
            "ours is behind"
        );

        let before = ino();
        h.push_pane_info(attached(&pane, AgentStatus::Idle));
        assert!(quick(&h, &mut run, "plan"));

        assert_eq!(ino(), before, "nothing to add → no write");
        assert_eq!(
            run.phases[0].pane_agent().and_then(|a| a.session()),
            Some(&want),
            "and the caller's copy still catches up, or it re-asks every poll"
        );
    }

    #[test]
    fn a_phase_missing_from_fresh_state_is_left_alone() {
        // A phase can vanish from disk between the poll and the write (another
        // writer rewrote the run). There is nothing to record it against, so the
        // capture is dropped rather than re-creating the phase — and the wait it
        // is running inside must survive it.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("vanished-on-capture");
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pane = run.phases[0].pane_id().map(str::to_owned).unwrap();

        let mut other = RunState::load("vanished-on-capture").unwrap();
        other.phases.clear();
        other.save().unwrap();

        h.push_pane_info(attached(&pane, AgentStatus::Idle));
        assert!(quick(&h, &mut run, "plan"), "the gate still works");
        assert!(
            RunState::load("vanished-on-capture")
                .unwrap()
                .phases
                .is_empty(),
            "capture must not resurrect a phase that is gone"
        );
    }

    #[test]
    fn a_phase_whose_marker_is_already_there_is_still_polled() {
        // The SAME skip the reviewer wait loop had, in the other loop. Asked
        // explicitly — "what else can skip this capture?" — this is what turned
        // up: `phase_wait`'s marker branch RETURNS (`Done`, `Superseded`), so a
        // phase whose marker was already on disk at the first iteration was
        // never polled at all, and a session is readable only while its agent
        // lives. That phase is then reaped with nothing to resume.
        //
        // The fix is the same: poll at the top of the loop, before any branch
        // that can return.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("wait-marker-first");
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pane = run.phases[0].pane_id().map(str::to_owned).unwrap();
        write_handoff(&run, "plan");
        let pass = run.phases[0].pass.clone().unwrap();

        // The agent finished before this wait ever started, and the ONLY poll
        // this wait will make is the one carrying the session.
        agent_signals_done(&run, "plan", &pass);
        h.push_pane_info(attached(&pane, AgentStatus::Idle));

        assert_eq!(
            phase_wait(&h, &mut run, "plan", 5000).unwrap(),
            PhaseWaitOutcome::Done,
            "the marker still completes the phase on the first look"
        );

        let want = SessionId::new(FakeHerdr::session_value_for(&pane)).unwrap();
        assert_eq!(
            RunState::load("wait-marker-first").unwrap().phases[0]
                .pane_agent()
                .and_then(|a| a.session()),
            Some(&want),
            "a marker already on disk must not skip the one poll this wait makes"
        );
        // And the caller holds it too — `Done` adopts the freshly loaded state,
        // which must include what the poll just wrote.
        assert_eq!(
            run.phases[0].pane_agent().and_then(|a| a.session()),
            Some(&want)
        );
    }

    #[test]
    fn phase_wait_also_keeps_a_session_herdr_stops_reporting() {
        // `a_session_survives_herdr_forgetting_it` proves the rule through the
        // readiness gate. This proves the OTHER loop applies it too — the one
        // that runs for the whole life of a phase, and so is the loop that
        // actually watches an agent exit. `POLL_INTERVAL` is not injectable
        // here, so the timeout is sized to buy exactly two polls.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("wait-keeps-session");
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pane = run.phases[0].pane_id().map(str::to_owned).unwrap();

        h.push_pane_info(attached(&pane, AgentStatus::Working));
        h.push_pane_info(session_less(&pane, AgentStatus::Unknown));

        assert_eq!(
            phase_wait(&h, &mut run, "plan", 700).unwrap(),
            PhaseWaitOutcome::TimedOut,
            "no marker was written, so this is an honest timeout"
        );
        let polls = h
            .calls()
            .iter()
            .filter(|c| c.contains("agent_status"))
            .count();
        assert!(polls >= 2, "the sequence needs both polls, saw {polls}");

        let want = SessionId::new(FakeHerdr::session_value_for(&pane)).unwrap();
        assert_eq!(
            RunState::load("wait-keeps-session").unwrap().phases[0]
                .pane_agent()
                .and_then(|a| a.session()),
            Some(&want),
            "the agent exiting must not take its session id with it"
        );
    }

    #[test]
    fn a_reviewers_session_is_captured_through_the_same_gate() {
        // Reviewers live in `review_phases`, are seeded through `phase_send`, and
        // are explicitly TOLD to exit — so they are the panes most certain to have
        // lost their session by the time anything wants it. The readiness gate is
        // the only poll they ever get.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("reviewer-capture");
        let phase = "review:task-1:1:correctness";
        spawn_reviewer(
            &h,
            &mut run,
            phase,
            None,
            &AgentLaunch::for_test("claude", "claude --permission-mode plan"),
        )
        .unwrap();
        let pane = run.review_phases[0].pane_id().map(str::to_owned).unwrap();

        phase_send(&h, &mut run, phase, "seed").unwrap();

        let want = SessionId::new(FakeHerdr::session_value_for(&pane)).unwrap();
        assert_eq!(
            run.review_phases[0].pane_agent().and_then(|a| a.session()),
            Some(&want),
            "a reviewer's session must be captured too"
        );
        assert_eq!(
            run.review_phases[0].tab_id.as_deref(),
            Some(FakeHerdr::tab_id_for(&pane).as_str()),
            "and its tab, which is what reaping it needs"
        );
        let on_disk = RunState::load("reviewer-capture").unwrap();
        assert_eq!(
            on_disk.review_phases[0]
                .pane_agent()
                .and_then(|a| a.session()),
            Some(&want)
        );
    }

    #[test]
    fn polling_a_phase_reads_that_phases_own_pane() {
        // `poll_phase_pane` used to take the phase and the pane as two separate
        // arguments, which let a caller hand over a phase and some OTHER phase's
        // pane. The capture would then attribute that pane's session to this
        // phase — permanently, because capture is one-shot: herdr drops the
        // session when the agent exits, so no later poll corrects it. Silent,
        // and reaping runs off the record it corrupted.
        //
        // The pane is derived from the phase now, so the mismatch is not
        // expressible. This pins that the derivation picks the RIGHT pane.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("derive-pane");
        phase_start(&h, &mut run, "plan", None).unwrap();
        phase_start(&h, &mut run, "implement", None).unwrap();
        let plan_pane = run.phases[0].pane_id().unwrap().to_owned();
        let impl_pane = run.phases[1].pane_id().unwrap().to_owned();
        assert_ne!(plan_pane, impl_pane, "two phases, two panes");

        poll_phase_pane(&h, &mut run, "implement");

        let polled: Vec<String> = h
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("pane_info"))
            .collect();
        assert_eq!(polled.len(), 1, "exactly one poll: {polled:?}");
        assert!(
            polled[0].contains(&impl_pane) && !polled[0].contains(&plan_pane),
            "must have read implement's pane, not plan's: {polled:?}"
        );

        // And the session landed on the phase that owns that pane.
        let want = SessionId::new(FakeHerdr::session_value_for(&impl_pane)).unwrap();
        assert_eq!(
            run.phases[1].pane_agent().and_then(|a| a.session()),
            Some(&want),
            "captured onto 'implement'"
        );
        assert!(
            run.phases[0]
                .pane_agent()
                .and_then(|a| a.session())
                .is_none(),
            "and NOT onto 'plan', which was never polled"
        );
    }

    #[test]
    fn a_session_is_never_recorded_without_the_launch_it_belongs_to() {
        // `Phase::record_session` refuses when there is no launch record, and
        // says so rather than silently dropping. A session is only meaningful
        // beside the backend that created it — that is the whole content of
        // `PhaseAgent` — so there is no half-built state to fall into.
        let mut p = Phase::new("plan");
        let id = SessionId::new("cca92f5b-3a8c".into()).unwrap();
        assert!(
            !p.record_session(id.clone()),
            "no launch record → refuse, and report it"
        );
        assert!(p.pane_agent().is_none(), "and record nothing");

        p.record_launch("cursor", Some("/cfg".into()));
        assert!(
            p.record_session(id.clone()),
            "with a launch record it lands"
        );
        let target = p.resume_target().expect("now resumable");
        assert_eq!(target.session(), &id);
        assert_eq!(target.backend(), "cursor");
        assert_eq!(target.profile(), Some("/cfg"));

        // A relaunch replaces the record — the one way a session is discarded.
        p.record_launch("cursor", Some("/cfg".into()));
        assert!(
            p.resume_target().is_none(),
            "a new launch is a new conversation; the old id is not this phase's"
        );
    }

    #[test]
    fn the_legacy_backend_fallback_reads_disk_not_the_callers_copy() {
        // A REPEAT of the round-2 attribution bug, one branch over. That round
        // fixed the phase-level source and left this legacy fallback resolving
        // from the caller's `run.agent` — which a long-running wait holds from
        // before the run's backend was known, or from a stale snapshot.
        //
        // The lesson, and why this test exists rather than just the fix: when
        // the rule is "attribute from what is on disk", sweep EVERY branch that
        // resolves the value, not the one the reviewer happened to cite.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("legacy-fallback-source");
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pane = run.phases[0].pane_id().unwrap().to_owned();

        // A legacy phase: no launch record, so the fallback decides. Disk says
        // the run is claude; the caller's stale copy says cursor.
        let mut disk = RunState::load("legacy-fallback-source").unwrap();
        disk.phases[0].clear_pane_agent_for_test();
        disk.agent = Some("claude".into());
        disk.save().unwrap();
        run.phases[0].clear_pane_agent_for_test();
        run.agent = Some("cursor".into());

        // herdr attributes the session to claude, because claude created it.
        h.push_pane_info(Some(PaneInfo {
            tab_id: FakeHerdr::tab_id_for(&pane),
            agent_status: Some(AgentStatus::Idle),
            agent_session: Some(FakeHerdr::session_for(&pane)),
        }));
        assert!(quick(&h, &mut run, "plan"));

        let want = SessionId::new(FakeHerdr::session_value_for(&pane)).unwrap();
        assert_eq!(
            RunState::load("legacy-fallback-source").unwrap().phases[0]
                .pane_agent()
                .and_then(|a| a.session()),
            Some(&want),
            "the fallback must attribute from the run state on DISK; the caller's \
             copy says cursor, which would refuse this claude session"
        );
    }

    #[test]
    fn a_reviewer_on_another_backend_still_gets_its_session_captured() {
        // `Config::review_agent_for` picks a reviewer's backend independently of
        // the run's — an explicit `review_agent`, or the cursor auto-selection.
        // Checking such a reviewer's session against `run.agent` would refuse it
        // by the resume rule's own (correct) logic, and silently never capture
        // the session of the pane MOST certain to have exited by the time
        // anything wants it. So the check is against the phase's own backend.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("cross-backend-reviewer");
        assert_eq!(run.agent.as_deref(), Some("claude"), "the RUN is claude");
        let phase = "review:task-1:1:correctness";
        spawn_reviewer(
            &h,
            &mut run,
            phase,
            None,
            &AgentLaunch::for_test("cursor", "cursor agent"),
        )
        .unwrap();
        let pane = run.review_phases[0].pane_id().map(str::to_owned).unwrap();
        assert_eq!(
            run.review_phases[0].pane_agent().map(|a| a.backend()),
            Some("cursor")
        );

        // herdr attributes the session to cursor, because cursor created it.
        h.push_pane_info(Some(PaneInfo {
            tab_id: FakeHerdr::tab_id_for(&pane),
            agent_status: Some(AgentStatus::Idle),
            agent_session: Some(FakeHerdr::session_owned_by(&pane, Some("cursor"))),
        }));
        assert!(quick(&h, &mut run, phase));

        let want = SessionId::new(FakeHerdr::session_value_for(&pane)).unwrap();
        assert_eq!(
            run.review_phases[0].pane_agent().and_then(|a| a.session()),
            Some(&want),
            "a cursor reviewer's session must be captured under cursor"
        );
    }

    #[test]
    fn a_launch_that_failed_does_not_leave_the_old_session_advertised() {
        // `phase_start` persists the new pass and `Running` BEFORE it touches the
        // pane, so a failed launch still leaves the phase on a pass no agent ever
        // ran under. If the session were cleared only after a successful launch,
        // that phase would sit there advertising the PREVIOUS pass's
        // conversation as resumable — the one state the clear exists to prevent,
        // reachable on the one path that skips it.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("failed-launch-session");
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pane = run.phases[0].pane_id().map(str::to_owned).unwrap();
        h.push_pane_info(attached(&pane, AgentStatus::Idle));
        assert!(quick(&h, &mut run, "plan"));
        let pass_a = run.phases[0].pass.clone().unwrap();
        assert!(
            run.phases[0]
                .pane_agent()
                .and_then(|a| a.session())
                .is_some(),
            "pass A has a session"
        );

        h.fail_pane_run();
        assert!(
            phase_start(&h, &mut run, "plan", None).is_err(),
            "the relaunch must fail"
        );

        let on_disk = RunState::load("failed-launch-session").unwrap();
        assert_ne!(
            on_disk.phases[0].pass,
            Some(pass_a),
            "a new pass was minted"
        );
        assert!(
            on_disk.phases[0]
                .pane_agent()
                .and_then(|a| a.session())
                .is_none(),
            "and pass A's conversation must not be advertised against it"
        );
    }

    #[test]
    fn a_new_pass_discards_the_previous_passs_session() {
        // `phase_start` re-runs the agent command in the SAME pane, so the pane's
        // conversation is replaced by a brand-new one. Keeping the old id would
        // point a resume at a conversation that is no longer this phase's — the
        // one case where clearing is right, and it clears on EVIDENCE of a
        // relaunch, never on a poll's silence.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("new-pass-session");
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pane = run.phases[0].pane_id().map(str::to_owned).unwrap();
        h.push_pane_info(attached(&pane, AgentStatus::Idle));
        assert!(quick(&h, &mut run, "plan"));
        assert!(
            run.phases[0]
                .pane_agent()
                .and_then(|a| a.session())
                .is_some()
        );

        phase_start(&h, &mut run, "plan", None).unwrap();

        assert!(
            run.phases[0]
                .pane_agent()
                .and_then(|a| a.session())
                .is_none(),
            "a relaunched pane holds a different conversation"
        );
        assert_eq!(
            run.phases[0].tab_id.as_deref(),
            Some(FakeHerdr::tab_id_for(&pane).as_str()),
            "the TAB is unchanged by a relaunch, so it is kept"
        );
        assert!(
            RunState::load("new-pass-session").unwrap().phases[0]
                .pane_agent()
                .and_then(|a| a.session())
                .is_none(),
            "and the stale id must not survive on disk either"
        );
    }

    #[test]
    fn a_lost_pass_token_does_not_discard_a_captured_session() {
        // `phase_wait`'s TokenLost branch re-enters on every poll. A vanished
        // pass token is a statement about drovr's own bookkeeping and says
        // nothing about whether the agent is alive — so it must not take the
        // session down with it.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("token-lost-capture");
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pane = run.phases[0].pane_id().map(str::to_owned).unwrap();
        let pass = run.phases[0].pass.clone().unwrap();
        write_handoff(&run, "plan");
        agent_signals_done(&run, "plan", &pass);

        // A lossy writer drops the token from disk; the waiter still holds it.
        let mut lossy = RunState::load("token-lost-capture").unwrap();
        lossy.phases[0].pass = None;
        lossy.save().unwrap();

        assert_eq!(
            phase_wait(&h, &mut run, "plan", 30).unwrap(),
            PhaseWaitOutcome::TimedOut,
            "a vanished token is not a supersession"
        );
        let want = SessionId::new(FakeHerdr::session_value_for(&pane)).unwrap();
        assert_eq!(
            RunState::load("token-lost-capture").unwrap().phases[0]
                .pane_agent()
                .and_then(|a| a.session()),
            Some(&want),
            "the session must be captured and kept through the TokenLost branch"
        );
    }

    #[test]
    fn a_launch_records_the_profile_it_authenticated_with() {
        // claude resolves a session id under
        // `$CLAUDE_CONFIG_DIR/projects/<escaped-cwd>/`, so an id captured under
        // one profile is invisible to a process holding another. The launch is
        // the only moment the profile is knowable: a later `drovr` may run from a
        // plain shell with none set.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("launch-profile");
        unsafe {
            std::env::set_var("CLAUDE_CONFIG_DIR", "/home/u/.config/claude-work");
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            phase_start(&h, &mut run, "plan", None).unwrap();
            spawn_reviewer(
                &h,
                &mut run,
                "review:task-1:1:correctness",
                None,
                &AgentLaunch::for_test("claude", "claude --permission-mode plan"),
            )
            .unwrap();
            assert_eq!(
                run.phases[0].pane_agent().and_then(|a| a.profile()),
                Some("/home/u/.config/claude-work")
            );
            assert_eq!(
                run.review_phases[0].pane_agent().and_then(|a| a.profile()),
                Some("/home/u/.config/claude-work"),
                "a reviewer is resumed the same way, so it needs the same profile"
            );
        }));
        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
        }
        result.unwrap();

        // No profile set → None, which means "whatever the default profile is",
        // exactly as the launch itself inlines nothing.
        let mut run = capture_run("launch-profile-default");
        phase_start(&h, &mut run, "plan", None).unwrap();
        assert!(
            run.phases[0]
                .pane_agent()
                .and_then(|a| a.profile())
                .is_none()
        );
    }

    /// Polling a phase's pane records what it sees; it never acts on it.
    ///
    /// This began as a task-scope guard ("nothing reaps yet"), and that reading
    /// expired when `phase_reap` landed. What it asserts did not: capture runs
    /// inside every wait loop, twice a second, on panes whose agents are very
    /// much alive — so a close reachable from here would be a close on no
    /// evidence at all. Reaping has its own triggers and its own tests
    /// (`phase::reap_tests`); this pins that the poll is not one of them.
    #[test]
    fn capture_never_reaps_a_pane() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = capture_run("no-reaping");
        phase_start(&h, &mut run, "plan", None).unwrap();
        let pane = run.phases[0].pane_id().map(str::to_owned).unwrap();
        h.push_pane_info(session_less(&pane, AgentStatus::Unknown));
        h.push_pane_info(attached(&pane, AgentStatus::Idle));
        assert!(quick(&h, &mut run, "plan"));

        assert!(!run.phases[0].is_reaped());
        assert_eq!(run.phases[0].pane_id(), Some(pane.as_str()));
        assert_eq!(run.phases[0].status, PhaseStatus::Running);
        assert!(
            !h.calls()
                .iter()
                .any(|c| c.contains("tab_close") || c.contains("pane_close")),
            "capture must never close anything: {:?}",
            h.calls()
        );
    }

    /// Local copies of `phase::tests`' helpers — sibling test modules do not
    /// share scope, and duplicating four lines beats making them `pub(super)`.
    fn write_handoff(run: &RunState, phase: &str) {
        let hp = run_dir(&run.name).join(format!("{phase}-HANDOFF.md"));
        std::fs::create_dir_all(hp.parent().unwrap()).unwrap();
        std::fs::write(&hp, "## Objective\nreal handoff\n").unwrap();
    }

    fn agent_signals_done(run: &RunState, phase: &str, token: &PassToken) {
        unsafe {
            std::env::set_var(PASS_ENV, token.as_str());
        }
        phase_done(run, phase).unwrap();
        unsafe {
            std::env::remove_var(PASS_ENV);
        }
    }
}

#[cfg(test)]
mod rehydrate_tests {
    use super::*;
    use crate::herdr::{FakeHerdr, SessionId};
    use crate::run::PhaseAgent;
    use crate::test_util::ENV_LOCK;

    /// A run whose config is the built-in map, whatever the developer's own
    /// `~/.config/drovr/config.toml` says. Rehydrate composes from config, so a
    /// user-configured `resume_flag` would otherwise decide these assertions.
    ///
    /// Returns the tempdir guard: dropping it removes the config home, so the
    /// caller must bind it.
    fn rehydrate_run(name: &str) -> (RunState, tempfile::TempDir) {
        // Caller must hold ENV_LOCK.
        let cfg_home = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", format!("/tmp/drovr-rehydrate-test-{name}"));
            std::env::set_var("XDG_CONFIG_HOME", cfg_home.path());
            std::env::remove_var(PASS_ENV);
            std::env::remove_var("CLAUDE_CONFIG_DIR");
        }
        let _ = std::fs::remove_dir_all(run_dir(name));
        let run = RunState {
            name: name.to_owned(),
            task: "test task".into(),
            agent: Some("claude".into()),
            phases: vec![],
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: Some("ws-rh".into()),
            root_pane: Some("ws-rh:root".into()),
            project_dir: "/tmp/drovr-proj-test".into(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
        };
        (run, cfg_home)
    }

    /// A phase that ran, was captured, and has since had its pane closed —
    /// exactly the state reaping leaves behind, and the one
    /// rehydrate exists to recover from.
    fn reaped_phase(
        name: &str,
        backend: &str,
        profile: Option<&str>,
        session: Option<&str>,
    ) -> Phase {
        let mut p = Phase::new(name);
        p.status = PhaseStatus::Done;
        p.set_pane("old-pane");
        p.record_launch(backend, profile.map(str::to_owned));
        if let Some(s) = session {
            assert!(
                p.record_session(SessionId::new(s.to_owned()).unwrap()),
                "fixture session must attach to the launch record"
            );
        }
        p.mark_reaped();
        p
    }

    /// Script `n` polls that report an agent up and carrying `session` — what
    /// herdr shows for a resume that really did land on its old conversation.
    ///
    /// Without this the fake reports a session DERIVED from the new pane id,
    /// which is what a backend that minted a fresh conversation would look
    /// like — and is exactly the case a resume must not call `Resumed`.
    fn script_resumed_session(h: &FakeHerdr, session: &str, backend: &str, n: usize) {
        for _ in 0..n {
            h.push_pane_info(Some(crate::herdr::PaneInfo {
                tab_id: FakeHerdr::tab_id_for("rehydrated"),
                agent_status: Some(crate::herdr::AgentStatus::Idle),
                agent_session: Some(FakeHerdr::session_valued(session, Some(backend))),
            }));
        }
    }

    fn pane_run_call(h: &FakeHerdr) -> String {
        h.calls()
            .into_iter()
            .find(|c| c.contains("pane_run"))
            .expect("rehydrate must launch something")
    }

    #[test]
    fn rehydrate_resumes_the_recorded_session_in_a_fresh_tab() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-resume");
        run.phases
            .push(reaped_phase("plan", "claude", Some("/tmp/prof"), Some("sess-abc")));
        run.save().unwrap();
        let h = FakeHerdr::new();
        // herdr reports the resumed conversation back — a resume is not
        // confirmed on readiness alone.
        script_resumed_session(&h, "sess-abc", "claude", 8);

        let outcome = phase_rehydrate(&h, &mut run, "plan").unwrap();
        assert!(
            matches!(outcome, RehydrateOutcome::Resumed),
            "{outcome:?}"
        );

        let calls = h.calls();
        let tab = calls
            .iter()
            .find(|c| c.contains("tab_create"))
            .expect("rehydrate creates its own tab, never the root pane");
        assert!(
            tab.contains("cwd=/tmp/drovr-proj-test"),
            "the run's project_dir decides where the session resolves: {tab}"
        );
        let launch = pane_run_call(&h);
        assert!(
            launch.contains("--resume 'sess-abc'"),
            "the id must be quoted and present: {launch}"
        );
        assert!(
            launch.contains("CLAUDE_CONFIG_DIR='/tmp/prof'"),
            "the RECORDED profile, not this process's: {launch}"
        );
        assert!(
            !launch.contains("--permission-mode plan"),
            "a pipeline phase is not read-only: {launch}"
        );

        let p = run.find_phase("plan").unwrap();
        assert!(!p.is_reaped(), "a phase with a live pane is not reaped");
        let pane = p.pane_id().expect("the new pane is recorded");
        assert_ne!(pane, "old-pane", "a fresh pane, not the closed one");
        // …and on disk, which is where every later command reads it.
        let on_disk = RunState::load("rh-resume").unwrap();
        assert_eq!(on_disk.find_phase("plan").unwrap().pane_id(), Some(pane));
        assert!(!on_disk.find_phase("plan").unwrap().is_reaped());
    }

    #[test]
    fn rehydrate_uses_the_recorded_profile_not_the_servers_environment() {
        // The review server is a long-lived daemon: its environment is whatever
        // it was started with, which need not be the profile the pane
        // authenticated under. claude resolves a session beneath
        // `$CLAUDE_CONFIG_DIR/projects/<escaped-cwd>/`, so reading the wrong one
        // silently finds NOTHING and the resume degrades to a blank agent.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-profile");
        unsafe {
            std::env::set_var("CLAUDE_CONFIG_DIR", "/tmp/the-servers-profile");
        }
        run.phases.push(reaped_phase(
            "plan",
            "claude",
            Some("/tmp/the-phases-profile"),
            Some("sess-p"),
        ));
        run.save().unwrap();
        let h = FakeHerdr::new();
        // herdr reports the resumed conversation back — a resume is not
        // confirmed on readiness alone.
        script_resumed_session(&h, "sess-p", "claude", 8);

        phase_rehydrate(&h, &mut run, "plan").unwrap();
        let launch = pane_run_call(&h);
        assert!(
            launch.contains("CLAUDE_CONFIG_DIR='/tmp/the-phases-profile'"),
            "{launch}"
        );
        assert!(
            !launch.contains("the-servers-profile"),
            "the process env must not leak in: {launch}"
        );

        // …and the same holds on the RESEED path, which has the LEAST recorded
        // information and so is the most tempting place to fall back to the
        // process environment. A phase that recorded no profile ran under the
        // DEFAULT one; inheriting the daemon's would silently move a reseeded
        // agent onto a different account from every other phase in the run.
        let h2 = FakeHerdr::new();
        let mut p = reaped_phase("replan", "claude", None, None); // no profile, no session
        p.handoff_doc = Some("/tmp/seed.md".into());
        run.phases.push(p);
        run.save().unwrap();
        phase_rehydrate(&h2, &mut run, "replan").unwrap();
        let relaunch = pane_run_call(&h2);
        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
        }
        assert!(
            !relaunch.contains("CLAUDE_CONFIG_DIR"),
            "a phase that recorded no profile is launched under the DEFAULT one, \
             not under whatever this process happens to hold: {relaunch}"
        );
    }

    #[test]
    fn a_reviewer_named_phase_cannot_smuggle_itself_past_the_reviewer_refusal() {
        // The reviewer refusal was answered by LIST MEMBERSHIP
        // (`review_phases.iter().any(…)`), not by identity. But
        // `review:<task>:<iter>:<angle>` is a legal `phase_start` name —
        // `require_name_unclaimed` checked `review_phases` only — so
        // `drovr phase start <run> review:…` registered one in `phases`, where
        // the membership test could not see it. Such a phase passed
        // `rehydratable`, showed the ⟳, and was relaunched with
        // `readonly=false` and no findings MCP: exactly the two things #9's
        // refusal exists to make impossible.
        //
        // Same class as #4 — two answers to one question — so it is fixed with
        // ONE predicate at both gates, not a third check.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-impostor");

        // (a) the creation gate refuses to mint one in the first place.
        let h = FakeHerdr::new();
        let err = phase_start(&h, &mut run, "review:task-1:1:security", None)
            .expect_err("a reviewer name belongs to the panel, not to phase start");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
        assert!(
            err.to_string().contains("code-review"),
            "must name what does spawn reviewers: {err}"
        );
        assert!(
            run.phases.iter().all(|p| p.name != "review:task-1:1:security"),
            "and nothing was registered: {:?}",
            run.phases.iter().map(|p| &p.name).collect::<Vec<_>>()
        );

        // (b) and one that got in by an older build is still refused, because
        //     the rehydrate gate asks the NAME, not which list it landed in.
        run.phases
            .push(reaped_phase("review:task-1:1:security", "claude", None, Some("s")));
        run.save().unwrap();
        assert_eq!(
            run.rehydratable("review:task-1:1:security"),
            Err(crate::run::NotRehydratable::Reviewer),
            "identity, not membership: {:?}",
            run.find_phase("review:task-1:1:security")
        );
        let err = phase_rehydrate(&h, &mut run, "review:task-1:1:security")
            .expect_err("an impostor reviewer must be refused too");
        assert!(err.to_string().contains("review-panel agent"), "{err}");
        assert!(
            !h.calls().iter().any(|c| c.contains("tab_create")),
            "nothing launched: {:?}",
            h.calls()
        );

        // (c) ⚠️ and the prefix must not swallow the PIPELINE phase `drovr new`
        //     seeds under the name "review". It is not `review:`-prefixed, it
        //     is an ordinary phase, and it stays rehydratable — a prefix test
        //     that got this wrong would silently remove the ⟳ from a real
        //     phase of every run.
        assert!(!crate::run::is_reviewer_phase_name("review"));
        run.phases.push(reaped_phase("review", "claude", None, Some("s2")));
        run.save().unwrap();
        assert_eq!(run.rehydratable("review"), Ok(()));
    }

    #[test]
    fn readonly_and_the_reviewer_refusal_answer_from_the_same_predicate() {
        // The gate said "reviewer" by NAME (`is_reviewer_phase_name`) while the
        // launch said it by LIST (`review_phases` membership). Two spellings of
        // one question, which is the drift the last two rounds each fixed once
        // — and it is reachable: a `review_phases` entry whose name lacks the
        // prefix passes the gate and then launched WITH `readonly_flag`, i.e.
        // as a reviewer the gate had just decided was not one.
        //
        // Such an entry is malformed state (`spawn_reviewer` cannot make one),
        // and the point is not which answer it gets but that it gets ONE.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-one-predicate");
        run.review_phases
            .push(reaped_phase("legacy-not-prefixed", "claude", None, Some("s")));
        run.save().unwrap();
        let h = FakeHerdr::new();
        script_resumed_session(&h, "s", "claude", 8);

        // The gate says "not a reviewer"…
        assert_eq!(run.rehydratable("legacy-not-prefixed"), Ok(()));
        phase_rehydrate_with_timeout(
            &h,
            &mut run,
            "legacy-not-prefixed",
            Duration::from_millis(50),
            Duration::from_millis(0), // no confirmation floor: keep the wait bounded in tests
            Duration::from_millis(1),
        )
        .unwrap();
        // …so the launch must not turn round and treat it as one.
        let launch = pane_run_call(&h);
        assert!(
            !launch.contains("--permission-mode plan"),
            "the gate and the launch must answer from one predicate: {launch}"
        );
    }

    #[test]
    fn a_reviewer_is_refused_rather_than_brought_back_unable_to_deliver() {
        // ⚠️ This test replaces `a_rehydrated_reviewer_is_still_read_only`,
        // which asserted that a rehydrated reviewer kept its `readonly_flag`.
        // It did — and it still would not have worked. A reviewer's ONLY job is
        // to deliver findings, and it delivers them through drovr's findings
        // MCP server, handed over on its command line at launch.
        // `Config::resume_launch` passes no `mcp_config`, so a resumed reviewer
        // has no `submit_findings` tool and `delivered_review` waits on a file
        // that can never appear. Bringing an agent back unable to do the one
        // thing it exists for is a worse outcome than refusing, so the
        // predicate refuses — and NOTHING is launched.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-reviewer");
        run.review_phases.push(reaped_phase(
            "review:task-1:1:security",
            "claude",
            None,
            Some("sess-rev"),
        ));
        run.save().unwrap();
        let h = FakeHerdr::new();

        let err = phase_rehydrate(&h, &mut run, "review:task-1:1:security")
            .expect_err("a reviewer cannot be rehydrated");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        let msg = err.to_string();
        assert!(
            msg.contains("review-panel agent") && msg.contains("drovr code-review run"),
            "the refusal has to name the thing that DOES work: {msg}"
        );
        assert!(
            !h.calls().iter().any(|c| c.contains("tab_create")),
            "a refusal must not open a tab first: {:?}",
            h.calls()
        );
    }

    #[test]
    fn a_subcommand_backend_is_resumed_after_the_command() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, cfg) = rehydrate_run("rh-subcommand");
        let dir = cfg.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "[agents.codex]\ncommand = \"codex\"\nresume_subcommand = \"resume\"\n",
        )
        .unwrap();
        run.agent = Some("codex".into());
        run.phases
            .push(reaped_phase("plan", "codex", None, Some("sess-cx")));
        run.save().unwrap();
        let h = FakeHerdr::new();
        // herdr reports the resumed conversation back — a resume is not
        // confirmed on readiness alone.
        script_resumed_session(&h, "sess-cx", "codex", 8);

        assert!(matches!(
            phase_rehydrate(&h, &mut run, "plan").unwrap(),
            RehydrateOutcome::Resumed
        ));
        let launch = pane_run_call(&h);
        assert!(
            launch.contains("codex resume 'sess-cx' "),
            "the subcommand binds to the command, before every flag: {launch}"
        );
    }

    #[test]
    fn no_session_reseeds_instead_of_emitting_a_bare_flag() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-noseed");
        let mut p = reaped_phase("plan", "claude", None, None);
        p.handoff_doc = Some("/tmp/seed.md".into());
        run.phases.push(p);
        run.save().unwrap();
        let h = FakeHerdr::new();

        let outcome = phase_rehydrate(&h, &mut run, "plan").unwrap();
        assert!(matches!(outcome, RehydrateOutcome::Reseeded), "{outcome:?}");

        let launch = pane_run_call(&h);
        assert!(
            !launch.contains("--resume"),
            "a bare --resume opens the session picker and parks the pane: {launch}"
        );
        // The seed is re-sent, so the fresh agent knows what phase it is in.
        let sent = h
            .calls()
            .into_iter()
            .find(|c| c.contains("agent_send"))
            .expect("the fallback must re-seed");
        assert!(sent.contains("/tmp/seed.md"), "{sent}");
        // The readiness gate polls, so the FRESH agent's session is captured on
        // the way past: a phase that has just been reseeded is immediately
        // rehydratable again rather than waiting for the next `phase wait`.
        let pane = run.find_phase("plan").unwrap().pane_id().unwrap().to_owned();
        assert_eq!(
            run.find_phase("plan")
                .unwrap()
                .pane_agent()
                .and_then(PhaseAgent::session)
                .map(|s| s.as_str().to_owned()),
            Some(FakeHerdr::session_value_for(&pane))
        );
    }

    #[test]
    fn a_backend_with_no_resume_surface_reseeds() {
        // codex ships with NEITHER resume field, deliberately: an unverified
        // guess at its argument order composes a wrong command line, where a
        // reseed merely costs the conversation.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-nosurface");
        run.agent = Some("codex".into());
        let mut p = reaped_phase("plan", "codex", None, Some("sess-cx"));
        p.handoff_doc = Some("/tmp/seed.md".into());
        run.phases.push(p);
        run.save().unwrap();
        let h = FakeHerdr::new();

        let outcome = phase_rehydrate(&h, &mut run, "plan").unwrap();
        assert!(matches!(outcome, RehydrateOutcome::Reseeded), "{outcome:?}");
        let launch = pane_run_call(&h);
        assert!(!launch.contains("resume"), "{launch}");
        assert!(launch.contains("codex"), "{launch}");
        // A relaunch REPLACES the agent record, so the id of a conversation
        // this pane is NOT in does not survive to be offered again. (Nothing
        // re-captures here: the fake's session is owned by claude, and this
        // phase runs codex.)
        assert_eq!(
            run.find_phase("plan")
                .unwrap()
                .pane_agent()
                .and_then(PhaseAgent::session),
            None,
            "the stale session must not outlive the agent that held it"
        );
    }

    #[test]
    fn rehydrate_refuses_a_phase_that_still_holds_a_pane() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-live");
        let mut p = Phase::new("plan");
        p.status = PhaseStatus::Running;
        p.set_pane("ws-rh:p7");
        p.record_launch("claude", None);
        run.phases.push(p);
        run.save().unwrap();
        let h = FakeHerdr::new();

        let err = phase_rehydrate(&h, &mut run, "plan")
            .expect_err("a phase with a live pane is not rehydrated");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
        assert!(err.to_string().contains("ws-rh:p7"), "{err}");
        // It must send the user at THAT pane, not at `drovr attach <run>`,
        // which resolves through `live_agent_pane` — and that skips `Done`
        // phases, i.e. exactly the ones rehydrate is asked about. It would
        // attach to a different phase, or deny any pane exists at all.
        assert!(
            err.to_string().contains("herdr pane read 'ws-rh:p7'"),
            "the refusal must name the pane it is talking about: {err}"
        );
        assert!(
            !err.to_string().contains("drovr attach"),
            "must not route through the Done-skipping resolver: {err}"
        );
        assert!(
            h.calls().is_empty(),
            "a refusal must not touch herdr: {:?}",
            h.calls()
        );
    }

    #[test]
    fn rehydrate_refuses_a_phase_that_never_ran() {
        // `drovr new` pre-seeds a run with Pending placeholders, so "the phase
        // is in state.json" is not "the phase has an agent to bring back".
        // Rehydrating one would launch a brand-new agent out of pipeline order
        // — under a command that says it recovers an old one.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-placeholder");
        run.phases.push(Phase::new("plan")); // Pending, no pane, never reaped
        run.save().unwrap();
        let h = FakeHerdr::new();

        let err = phase_rehydrate(&h, &mut run, "plan")
            .expect_err("a placeholder is not rehydratable");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
        assert!(
            err.to_string().contains("drovr phase start 'rh-placeholder' 'plan'"),
            "must name the command that IS right, ready to paste: {err}"
        );
        assert!(h.calls().is_empty(), "nothing launched: {:?}", h.calls());

        // …and the same phase becomes rehydratable the moment it has run.
        // Saved, not merely mutated: `phase_rehydrate` re-reads `state.json`
        // under its lock, so an in-memory-only change is not a change at all.
        run.phases[0].status = PhaseStatus::Running;
        run.phases[0].record_launch("claude", None);
        run.save().unwrap();
        assert!(phase_rehydrate(&h, &mut run, "plan").is_ok());
    }

    #[test]
    fn a_resume_is_not_confirmed_until_the_session_itself_comes_back() {
        // The next layer of "it launched is not it worked". The readiness gate
        // proves an agent is UP on the new pane; it proves nothing about WHICH
        // conversation it is in. A recorded id that no longer resolves makes
        // claude start a fresh session and sit there looking perfectly healthy —
        // idle, attached, ready — and the ⟳'s entire promise is that the actual
        // conversation returns. Reporting `Resumed` for a stranger is the one
        // lie that promise cannot afford.
        //
        // The fake reports a session DERIVED from the pane, so a relaunch onto a
        // new pane yields a DIFFERENT id — precisely what a fresh conversation
        // looks like from the outside.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-other-session");
        run.phases
            .push(reaped_phase("plan", "claude", None, Some("sess-abc")));
        run.save().unwrap();
        let h = FakeHerdr::new();

        let outcome = phase_rehydrate_with_timeout(
            &h,
            &mut run,
            "plan",
            Duration::from_millis(20),
            Duration::from_millis(0), // no confirmation floor: keep the wait bounded in tests
            Duration::from_millis(1),
        )
        .unwrap();

        // The resume WAS composed and the agent DID come up — this is not the
        // never-ready case and not a fallback to reseed.
        assert!(pane_run_call(&h).contains("--resume 'sess-abc'"));
        let RehydrateOutcome::Incomplete(why) = &outcome else {
            panic!("a session that did not come back is not a resume: {outcome:?}");
        };
        let Unfinished::ResumeContradicted {
            expected, observed, ..
        } = why
        else {
            panic!("the reason must say WHICH failure this is: {why:?}");
        };
        assert_eq!(expected.as_str(), "sess-abc");
        assert_ne!(
            observed.as_str(),
            "sess-abc",
            "the id herdr DID report is the diagnostic — and it being a SESSION \
             rather than an Option is what makes this arm the one that surrenders"
        );
        let note = why.note("rh-other-session", "plan");
        assert!(
            note.contains("sess-abc") && note.contains("conversation"),
            "the human has to be told the conversation is not back: {note}"
        );
    }

    #[test]
    fn an_unconfirmed_resume_keeps_the_session_it_was_trying_to_confirm() {
        // ⚠️ THE TOKEN A RETRY NEEDS. Both waits on the resume path poll through
        // `poll_phase_pane`, which captures — and the pane they are polling is
        // the NEW one, whose agent reports a DIFFERENT session. Capture wrote
        // that over `sess-abc` and saved it, so the honest exit-2 outcome came
        // back with the state WORSE than before the attempt: the recorded
        // resume token gone, and every later rehydrate composing `--resume` for
        // a stranger's conversation instead of reseeding from the handoff.
        //
        // §4's lesson one layer out: if this fails, what did I already take
        // away? Here — the one value the whole operation exists to use.
        //
        // Asserted on DISK, because that is where the damage was.
        let _lock = ENV_LOCK.lock().unwrap();

        // (a) the agent comes up, in someone else's conversation.
        let (mut run, _cfg) = rehydrate_run("rh-keep-unconfirmed");
        run.phases
            .push(reaped_phase("plan", "claude", None, Some("sess-abc")));
        run.save().unwrap();
        let h = FakeHerdr::new();
        let outcome = phase_rehydrate_with_timeout(
            &h,
            &mut run,
            "plan",
            Duration::from_millis(20),
            Duration::from_millis(0), // no confirmation floor: keep the wait bounded in tests
            Duration::from_millis(1),
        )
        .unwrap();
        assert!(
            matches!(outcome, RehydrateOutcome::Incomplete(Unfinished::ResumeContradicted { .. })),
            "{outcome:?}"
        );
        let on_disk = RunState::load("rh-keep-unconfirmed").unwrap();
        assert_eq!(
            on_disk
                .find_phase("plan")
                .and_then(|p| p.resume_target())
                .map(|t| t.session().as_str().to_owned()),
            Some("sess-abc".to_string()),
            "the id the resume was CONFIRMING must survive failing to confirm it: {:?}",
            on_disk.find_phase("plan")
        );

        // (b) and the same when the agent never comes up at all — that path
        // never reaches the confirmation loop, so it is a separate hole.
        let (mut run, _cfg2) = rehydrate_run("rh-keep-never-ready");
        run.phases
            .push(reaped_phase("plan", "claude", None, Some("sess-stale")));
        run.save().unwrap();
        let h2 = FakeHerdr::new();
        for _ in 0..40 {
            h2.push_status(Some("blocked"));
        }
        let outcome = phase_rehydrate_with_timeout(
            &h2,
            &mut run,
            "plan",
            Duration::from_millis(20),
            Duration::from_millis(0), // no confirmation floor: keep the wait bounded in tests
            Duration::from_millis(1),
        )
        .unwrap();
        assert!(
            matches!(
                outcome,
                RehydrateOutcome::Incomplete(Unfinished::NeverReady { resuming: true, .. })
            ),
            "{outcome:?}"
        );
        let on_disk = RunState::load("rh-keep-never-ready").unwrap();
        assert_eq!(
            on_disk
                .find_phase("plan")
                .and_then(|p| p.resume_target())
                .map(|t| t.session().as_str().to_owned()),
            Some("sess-stale".to_string()),
            "{:?}",
            on_disk.find_phase("plan")
        );
    }

    #[test]
    fn an_unconfirmed_resume_gives_its_pane_back_so_the_phase_can_be_retried() {
        // The other half of the same defect. A resume deliberately does NOT
        // `record_launch` — the conversation is meant to be the same one — so
        // when it is not, the phase is left recording a pane whose agent its
        // own `pane_agent` does not describe: a live pane running a stranger,
        // attributed to a conversation that is somewhere else.
        //
        // That is "never live-but-unrecorded" (round 2) one notch stricter:
        // never live-but-MISATTRIBUTED. And while it stands, `HoldsPane`
        // refuses every retry, so a rehydrate that failed cannot be attempted
        // again.
        //
        // The asymmetry is the whole rule: surrender needs POSITIVE evidence
        // of misattribution. Here `record_launch` DID run, so the record
        // describes the pane, the note tells the operator to `phase send` it,
        // and closing it would destroy the very agent they were pointed at.
        // `NeverReady { resuming: true }` keeps its pane for the other half of
        // the rule — drovr observed nothing, and nothing is not evidence.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-give-back");
        run.phases
            .push(reaped_phase("plan", "claude", None, Some("sess-abc")));
        run.save().unwrap();
        let h = FakeHerdr::new();

        let outcome = phase_rehydrate_with_timeout(
            &h,
            &mut run,
            "plan",
            Duration::from_millis(20),
            Duration::from_millis(0), // no confirmation floor: keep the wait bounded in tests
            Duration::from_millis(1),
        )
        .unwrap();
        let RehydrateOutcome::Incomplete(why) = &outcome else {
            panic!("{outcome:?}");
        };
        let pane = why.pane().to_owned();

        let on_disk = RunState::load("rh-give-back").unwrap();
        assert_eq!(
            on_disk.rehydratable("plan"),
            Ok(()),
            "a failed rehydrate must be retryable: {:?}",
            on_disk.find_phase("plan")
        );
        assert!(
            on_disk.retired_panes.iter().any(|p| p == &pane),
            "and cleanup must still be able to prove the pane was drovr's: {:?}",
            on_disk.retired_panes
        );
        assert!(
            h.calls().iter().any(|c| c == &format!("pane_close pane={pane}")),
            "{:?}",
            h.calls()
        );
        // …and the retry it enables is a MEANINGFUL one: the token survived.
        assert_eq!(
            on_disk
                .find_phase("plan")
                .and_then(|p| p.resume_target())
                .map(|t| t.session().as_str().to_owned()),
            Some("sess-abc".to_string()),
        );
        // The note must not send the operator to a pane that is gone.
        let note = why.note("rh-give-back", "plan");
        assert!(
            !note.contains("herdr pane read"),
            "the pane was closed; do not advertise reading it: {note}"
        );
    }

    #[test]
    fn a_slow_launch_cannot_starve_the_step_that_confirms_the_session() {
        // Readiness and confirmation shared ONE budget, so a slow start ate
        // most of it and left confirmation a sliver — making "no session seen"
        // most likely exactly when the machine is loaded, i.e. when the agent
        // is slowest to surface its id. That reports a resume which actually
        // worked as unconfirmed, exit 2.
        //
        // Driven with a spent readiness budget and an explicit floor, so the
        // wiring is pinned deterministically rather than by racing a clock: the
        // agent is up on the FIRST poll (readiness returns before it ever tests
        // the deadline), which leaves the shared budget already gone.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-starved-confirm");
        run.phases
            .push(reaped_phase("plan", "claude", None, Some("sess-abc")));
        run.save().unwrap();
        let h = FakeHerdr::new();
        // Up, but no session yet — the one-poll lag that is the whole reason
        // confirmation needs time of its own.
        h.push_pane_info(Some(crate::herdr::PaneInfo {
            tab_id: FakeHerdr::tab_id_for("rehydrated"),
            agent_status: Some(crate::herdr::AgentStatus::Idle),
            agent_session: None,
        }));
        script_resumed_session(&h, "sess-abc", "claude", 8);

        let outcome = phase_rehydrate_with_timeout(
            &h,
            &mut run,
            "plan",
            Duration::from_millis(0), // the shared budget is spent on arrival
            Duration::from_millis(200), // …and confirmation still gets its floor
            Duration::from_millis(1),
        )
        .unwrap();
        assert_eq!(
            outcome,
            RehydrateOutcome::Resumed,
            "a launch that used the whole budget must not decide the answer"
        );
    }

    #[test]
    fn a_resume_that_never_reports_any_session_keeps_its_pane_too() {
        // ⚠️ THE ROUND-4 RULE, APPLIED ONE GATE LATER. The surrender fired on
        // every unconfirmed resume, including the one where herdr reported NO
        // session at all within the budget. That is the same epistemic state
        // `NeverReady` is in — drovr saw nothing — one readiness gate further
        // on, and it was destroying a pane on exactly the evidence the rule
        // says may not authorise it.
        //
        // The window is real, not theoretical: herdr can report `Idle` while
        // `agent_session` is still absent, which is precisely what
        // `a_session_that_shows_up_a_poll_late_is_still_a_confirmed_resume`
        // covers from the other side — there the id eventually arrives; here
        // the budget runs out first.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-no-session-seen");
        run.phases
            .push(reaped_phase("plan", "claude", None, Some("sess-abc")));
        run.save().unwrap();
        let h = FakeHerdr::new();
        // Up and healthy, but herdr never surfaces a session.
        for _ in 0..80 {
            h.push_pane_info(Some(crate::herdr::PaneInfo {
                tab_id: FakeHerdr::tab_id_for("rehydrated"),
                agent_status: Some(crate::herdr::AgentStatus::Idle),
                agent_session: None,
            }));
        }

        let outcome = phase_rehydrate_with_timeout(
            &h,
            &mut run,
            "plan",
            Duration::from_millis(20),
            Duration::from_millis(0), // no confirmation floor: keep the wait bounded in tests
            Duration::from_millis(1),
        )
        .unwrap();

        let RehydrateOutcome::Incomplete(why) = &outcome else {
            panic!("a resume drovr could not confirm is not a success: {outcome:?}");
        };
        assert!(
            matches!(why, Unfinished::ResumeUnobserved { .. }),
            "seeing NOTHING is its own answer, not a contradiction: {why:?}"
        );
        assert!(
            !run.find_phase("plan").unwrap().is_reaped(),
            "nothing was observed, so nothing may be destroyed: {:?}",
            run.find_phase("plan")
        );
        assert!(
            !h.calls().iter().any(|c| c.starts_with("pane_close")),
            "{:?}",
            h.calls()
        );
        // …and the token is still there either way.
        let on_disk = RunState::load("rh-no-session-seen").unwrap();
        assert_eq!(
            on_disk
                .find_phase("plan")
                .and_then(|p| p.resume_target())
                .map(|t| t.session().as_str().to_owned()),
            Some("sess-abc".to_string()),
        );
        // The note must send the operator to the pane that is still there, and
        // say what a retry will run into.
        let note = why.note("rh-no-session-seen", "plan");
        let pane = run.find_phase("plan").unwrap().pane_id().unwrap();
        assert!(note.contains(pane), "{note}");
        assert!(note.contains("still holds pane"), "{note}");
    }

    #[test]
    fn a_reseed_that_is_incomplete_keeps_its_pane_because_the_record_fits_it() {
        // The control for the test above, and the reason the rule is stated as
        // "does the record describe the agent in the pane" rather than
        // "did it fail". A reseed calls `record_launch`, so the phase's record
        // DOES describe the fresh agent — the pane is the phase's, the note
        // tells the operator to `phase send` it, and taking it away would
        // destroy a working agent to satisfy a tidiness rule.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-keep-reseed");
        let mut p = reaped_phase("plan", "claude", None, None); // no session → reseed
        p.handoff_doc = Some("/tmp/seed.md".into());
        run.phases.push(p);
        run.save().unwrap();
        let h = FakeHerdr::new();
        for _ in 0..40 {
            h.push_status(Some("blocked")); // never becomes ready
        }

        let outcome = phase_rehydrate_with_timeout(
            &h,
            &mut run,
            "plan",
            Duration::from_millis(20),
            Duration::from_millis(0), // no confirmation floor: keep the wait bounded in tests
            Duration::from_millis(1),
        )
        .unwrap();
        assert!(
            matches!(
                outcome,
                RehydrateOutcome::Incomplete(Unfinished::NeverReady { resuming: false, .. })
            ),
            "{outcome:?}"
        );
        let on_disk = RunState::load("rh-keep-reseed").unwrap();
        assert!(
            !on_disk.find_phase("plan").unwrap().is_reaped(),
            "the pane is the phase's own agent and stays: {:?}",
            on_disk.find_phase("plan")
        );
        assert!(
            !h.calls().iter().any(|c| c.starts_with("pane_close")),
            "nothing is closed on the reseed path: {:?}",
            h.calls()
        );
    }

    #[test]
    fn a_resume_whose_session_comes_back_is_reported_resumed() {
        // The control for the test above: when herdr reports the agent carrying
        // the id it was told to resume, that IS the conversation coming back,
        // and it must still be reported as such. Without this, "never claim
        // Resumed" would pass trivially.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-confirmed");
        run.phases
            .push(reaped_phase("plan", "claude", None, Some("sess-abc")));
        run.save().unwrap();
        let h = FakeHerdr::new();
        script_resumed_session(&h, "sess-abc", "claude", 8);

        let outcome = phase_rehydrate_with_timeout(
            &h,
            &mut run,
            "plan",
            Duration::from_millis(50),
            Duration::from_millis(0), // no confirmation floor: keep the wait bounded in tests
            Duration::from_millis(1),
        )
        .unwrap();
        assert_eq!(outcome, RehydrateOutcome::Resumed, "{outcome:?}");
    }

    #[test]
    fn a_session_that_shows_up_a_poll_late_is_still_a_confirmed_resume() {
        // The readiness gate returns on the FIRST poll that reports "started",
        // and herdr does not necessarily carry an `agent_session` that early —
        // the same one-poll lag that cost reviewers their captured sessions.
        // Confirmation must therefore keep looking until the deadline rather
        // than judging the resume on a single sample.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-late-session");
        run.phases
            .push(reaped_phase("plan", "claude", None, Some("sess-abc")));
        run.save().unwrap();
        let h = FakeHerdr::new();
        // Up, but no session yet.
        h.push_pane_info(Some(crate::herdr::PaneInfo {
            tab_id: FakeHerdr::tab_id_for("rehydrated"),
            agent_status: Some(crate::herdr::AgentStatus::Idle),
            agent_session: None,
        }));
        script_resumed_session(&h, "sess-abc", "claude", 8);

        let outcome = phase_rehydrate_with_timeout(
            &h,
            &mut run,
            "plan",
            Duration::from_millis(200),
            Duration::from_millis(0), // no confirmation floor: keep the wait bounded in tests
            Duration::from_millis(1),
        )
        .unwrap();
        assert_eq!(outcome, RehydrateOutcome::Resumed, "{outcome:?}");
    }

    #[test]
    fn a_resume_that_never_comes_up_is_not_reported_as_resumed() {
        // `pane_run` returning Ok means the command was ISSUED. A recorded
        // session id that no longer resolves — pruned session file, cleared
        // profile storage, a backend that changed its id format — launches,
        // finds no conversation, and errors out or parks. Reporting `Resumed`
        // there would claim "same conversation, same agent" on the strength of
        // a spawn, and hand a driver exit 0 for an agent that was never resumed.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-resume-dead");
        run.phases
            .push(reaped_phase("plan", "claude", None, Some("sess-stale")));
        run.save().unwrap();
        let h = FakeHerdr::new();
        for _ in 0..40 {
            h.push_status(Some("blocked")); // never reaches a started state
        }

        let outcome = phase_rehydrate_with_timeout(
            &h,
            &mut run,
            "plan",
            Duration::from_millis(20),
            Duration::from_millis(0), // no confirmation floor: keep the wait bounded in tests
            Duration::from_millis(1),
        )
        .unwrap();

        // The resume WAS composed — this is not a fallback to reseed.
        assert!(pane_run_call(&h).contains("--resume 'sess-stale'"));
        let RehydrateOutcome::Incomplete(why) = &outcome else {
            panic!("an unconfirmed resume must not report as Resumed: {outcome:?}");
        };
        assert!(
            matches!(why, Unfinished::NeverReady { resuming: true, .. }),
            "the variant IS the classification: {why:?}"
        );
        let note = why.note("rh-resume-dead", "plan");
        assert!(
            note.contains("conversation was NOT restored"),
            "must say what did not happen, in the resume's own terms: {note}"
        );
        // ⚠️ THE PANE IS KEPT, and which failures may destroy one is the whole
        // point of this assertion.
        //
        // `NeverReady` means drovr observed NOTHING — the agent reported no
        // status at all. "I don't know" must not authorise destroying a pane,
        // and this branch's founding rule is exactly that: the marker is the
        // evidence, absence of confirmation is not evidence. The common cause
        // is the recoverable one — the agent parked on a first-run or
        // permission prompt, a human clicks through, and the resumed
        // conversation is fine. drovr already treats that as human-recoverable
        // (`phase wait` exit 4, `diagnose_stuck_phase`); closing the pane
        // throws it away and cannot be undone.
        //
        // `ResumeContradicted` is the opposite and DOES surrender: there drovr
        // positively observed a DIFFERENT session, so the record demonstrably
        // does not describe that pane. `ResumeUnobserved` — up, but no session
        // reported — sits on THIS side of the line with `NeverReady`. See
        // `an_unconfirmed_resume_gives_its_pane_back_so_the_phase_can_be_retried`.
        assert!(
            !run.find_phase("plan").unwrap().is_reaped(),
            "an unobserved agent is not a misattributed one: {:?}",
            run.find_phase("plan")
        );
        assert!(
            !h.calls().iter().any(|c| c.starts_with("pane_close")),
            "nothing may be closed on no evidence: {:?}",
            h.calls()
        );
        let pane = run.find_phase("plan").unwrap().pane_id().unwrap().to_owned();
        assert!(
            note.contains(&pane) && note.contains("herdr pane read"),
            "and the operator must be sent to the pane that is still there: {note}"
        );
    }

    #[test]
    fn a_phase_with_no_recorded_seed_says_so_rather_than_blaming_a_send() {
        // `phase_start` takes `seed: Option<&Path>`, so a phase legitimately has
        // none — and this branch had NO coverage at all, which is how the note
        // came to claim "its seed was NOT re-sent" for a phase that never had
        // one to send.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-no-seed");
        // No session (→ reseed) and no handoff_doc (→ nothing to reseed WITH).
        run.phases.push(reaped_phase("plan", "claude", None, None));
        run.save().unwrap();
        let h = FakeHerdr::new();

        // (a) the agent comes up fine: the honest answer is "there was no seed".
        let outcome = phase_rehydrate(&h, &mut run, "plan").unwrap();
        let RehydrateOutcome::Incomplete(why) = &outcome else {
            panic!("a fresh agent with no context is not a success: {outcome:?}");
        };
        assert!(
            matches!(why, Unfinished::NoSeed { .. }),
            "there was nothing to send, and the variant says so: {why:?}"
        );
        let note = why.note("rh-no-seed", "plan");
        assert!(note.contains("no recorded seed document"), "{note}");
        assert!(
            note.contains("drovr phase send 'rh-no-seed' 'plan'"),
            "must name the way to give it context: {note}"
        );
        assert!(
            !h.calls().iter().any(|c| c.contains("agent_send")),
            "there was nothing to send: {:?}",
            h.calls()
        );

        // (b) and when it ALSO never becomes ready, the note must not blame a
        // send that never happened.
        let (mut run, _cfg2) = rehydrate_run("rh-no-seed-stuck");
        run.phases.push(reaped_phase("plan", "claude", None, None));
        run.save().unwrap();
        let h2 = FakeHerdr::new();
        for _ in 0..40 {
            h2.push_status(Some("blocked"));
        }
        let outcome = phase_rehydrate_with_timeout(
            &h2,
            &mut run,
            "plan",
            Duration::from_millis(20),
            Duration::from_millis(0), // no confirmation floor: keep the wait bounded in tests
            Duration::from_millis(1),
        )
        .unwrap();
        let RehydrateOutcome::Incomplete(why) = &outcome else {
            panic!("{outcome:?}");
        };
        assert!(
            matches!(
                why,
                Unfinished::NeverReady {
                    resuming: false,
                    had_seed: false,
                    ..
                }
            ),
            "never-ready AND seedless — two facts, both in the variant: {why:?}"
        );
        let note = why.note("rh-no-seed-stuck", "plan");
        assert!(
            note.contains("no recorded seed document either"),
            "must not claim a seed failed to send when there was none: {note}"
        );
        assert!(
            !note.contains("seed was NOT re-sent"),
            "that is a different failure: {note}"
        );
    }

    #[test]
    fn a_reseed_that_cannot_be_delivered_names_the_pane_it_just_made() {
        // The mainline failure of rehydrate's OWN recovery: the fresh agent is
        // up but never becomes ready (a first-run or permission prompt), so the
        // seed is not delivered. The note must point at the pane this call just
        // created — `drovr attach <run>` would NOT find it, because
        // `live_agent_pane` skips `Done` phases and `Done` is the usual reason a
        // phase was reaped in the first place.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-not-ready");
        let mut p = reaped_phase("plan", "claude", None, None); // no session → reseed
        p.handoff_doc = Some("/tmp/seed.md".into());
        run.phases.push(p);
        run.save().unwrap();
        let h = FakeHerdr::new();
        // An agent that never reports a started status: the readiness gate polls
        // it out. `Blocked` is the real-world shape — parked on a prompt.
        for _ in 0..40 {
            h.push_status(Some("blocked"));
        }

        let outcome = phase_rehydrate_with_timeout(
            &h,
            &mut run,
            "plan",
            Duration::from_millis(20),
            Duration::from_millis(0), // no confirmation floor: keep the wait bounded in tests
            Duration::from_millis(1),
        )
        .unwrap();

        let pane = run.find_phase("plan").unwrap().pane_id().unwrap().to_owned();
        let RehydrateOutcome::Incomplete(why) = &outcome else {
            panic!("an undeliverable seed must not report as Reseeded: {outcome:?}");
        };
        assert!(
            matches!(
                why,
                Unfinished::NeverReady {
                    resuming: false,
                    had_seed: true,
                    ..
                }
            ),
            "{why:?}"
        );
        assert_eq!(why.pane(), pane, "every arm names a pane, and it is this one");
        let note = why.note("rh-not-ready", "plan");
        assert!(note.contains(&pane), "must name the pane it created: {note}");
        assert!(
            note.contains(&format!("herdr pane read '{pane}'")),
            "ready to paste, and readable even if no agent ever attached: {note}"
        );
        assert!(
            !note.contains("drovr attach"),
            "must not route through the Done-skipping resolver: {note}"
        );
        assert!(note.contains("NOT re-sent"), "must be honest about the seed: {note}");
        // The pane really is live and recorded — only the seed did not land.
        assert!(!run.find_phase("plan").unwrap().is_reaped());
        assert!(
            !h.calls().iter().any(|c| c.contains("agent_send")),
            "nothing was sent to an agent that never became ready: {:?}",
            h.calls()
        );
    }

    #[test]
    fn an_unremovable_marker_does_not_fail_a_rehydrate_that_worked() {
        // The inverse of the evidence bug, and a regression the reordering
        // introduced: by the time the marker is swept, the agent is running and
        // its pane is durably recorded. A hard error there reports a rehydrate
        // that fully SUCCEEDED as a failure — exit 1, or an HTTP 500 claiming
        // nothing happened about a live pane — and the retry that invites is
        // then refused with `HoldsPane`, sending the operator to look at a pane
        // they were just told did not exist.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-stuck-marker");
        run.phases
            .push(reaped_phase("plan", "claude", None, Some("sess-sm")));
        run.save().unwrap();
        // A DIRECTORY where the marker file goes: `remove_file` cannot remove it.
        std::fs::create_dir_all(done_marker("rh-stuck-marker", "plan")).unwrap();
        let h = FakeHerdr::new();
        // herdr reports the resumed conversation back — a resume is not
        // confirmed on readiness alone.
        script_resumed_session(&h, "sess-sm", "claude", 8);

        let outcome = phase_rehydrate(&h, &mut run, "plan")
            .expect("a live, recorded pane is not a failed rehydrate");
        assert!(matches!(outcome, RehydrateOutcome::Resumed), "{outcome:?}");

        // The pane is real and recorded; only the (inert) file lingers.
        let on_disk = RunState::load("rh-stuck-marker").unwrap();
        let phase = on_disk.find_phase("plan").unwrap();
        assert!(phase.pane_id().is_some(), "the pane must be recorded");
        assert!(!phase.is_reaped());
        // Inert: the phase now holds a pass the leftover marker cannot match, so
        // `phase_wait` is correct whether or not the sweep worked.
        let pass = phase.pass.clone().expect("a new pass was minted");
        assert!(
            !pass.matches_marker(""),
            "an untokenized leftover cannot complete a tokened phase"
        );
    }

    #[test]
    fn surrendering_a_pane_records_the_retirement_and_not_the_phase() {
        // The half of the unrecordable-pane fix that the read-only-directory
        // test cannot reach: when the retirement DOES land, it must land on
        // disk-as-it-was, never on the caller's copy.
        //
        // The caller's copy is the one whose save just failed, so it has the
        // phase pointing at the pane this function is about to close. Writing
        // that would leave a phase claiming a pane that no longer exists —
        // `rehydratable` then answers `HoldsPane` forever and nothing clears
        // the registration, which is a phase no rehydrate can ever recover.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-surrender");
        run.phases
            .push(reaped_phase("plan", "claude", None, Some("sess-s")));
        run.save().unwrap();
        // Mutate in memory ONLY — exactly the state a failed post-launch save
        // leaves behind.
        run.find_phase_mut("plan").unwrap().set_pane("ws-rh:doomed");
        let h = FakeHerdr::new();

        surrender_unrecordable_pane(&h, &run, "ws-rh:doomed");

        let on_disk = RunState::load("rh-surrender").unwrap();
        let p = on_disk.find_phase("plan").unwrap();
        assert_eq!(
            p.pane_id(),
            None,
            "a closed pane must never be recorded as the phase's: {p:?}"
        );
        assert!(p.is_reaped(), "and the phase is untouched, so a retry works");
        assert!(
            on_disk.retired_panes.iter().any(|x| x == "ws-rh:doomed"),
            "but cleanup must still be able to prove the pane was drovr's: {:?}",
            on_disk.retired_panes
        );
        assert!(
            h.calls().iter().any(|c| c == "pane_close pane=ws-rh:doomed"),
            "{:?}",
            h.calls()
        );
    }

    #[test]
    fn a_pane_that_cannot_be_released_says_what_is_true_not_what_would_be_convenient() {
        // ⚠️ AN HONEST EXIT CODE WITH WRONG GUIDANCE IS NOT HONEST.
        //
        // The surrender closes the pane and then records the release. If that
        // record does not land, disk still names a `pane_id` for a pane that no
        // longer exists, so `rehydratable` answers `HoldsPane` FOREVER — while
        // the outcome this used to return said the phase was unchanged and
        // could be rehydrated again, and the warning implied the refusal would
        // lift once the run dir was writable. It never does: writability does
        // not clear `pane_id`; only a successful save would.
        //
        // So the failure is an `Err`, and its text names the one thing that
        // actually clears it.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-stuck-release");
        let mut p = reaped_phase("plan", "claude", None, Some("sess-abc"));
        p.set_pane("ws-rh:doomed");
        run.phases.push(p);
        run.save().unwrap();
        let h = FakeHerdr::new();

        use std::os::unix::fs::PermissionsExt;
        let dir = run_dir("rh-stuck-release");
        let before = std::fs::metadata(&dir).unwrap().permissions();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let released = surrender_misattributed_pane(&h, &mut run, "plan", "ws-rh:doomed");
        std::fs::set_permissions(&dir, before).unwrap();

        let err = released.expect_err("a release that did not land is not a success");
        let msg = err.to_string();
        assert!(
            msg.contains("ws-rh:doomed") && msg.contains("drovr phase reap"),
            "must name the pane that is stuck and the COMMAND that clears it — this \
             used to end in a hand-edit of state.json, and `drovr phase reap` is the \
             supported repair now: {msg}"
        );
        assert!(
            !msg.contains("can be rehydrated again") && !msg.contains("will retry"),
            "must not promise a retry the recorded state will refuse: {msg}"
        );
        // The pane is closed either way: leaving it open would add an immortal
        // pane to an already-stuck registration — two manual repairs, not one.
        assert!(
            h.calls().iter().any(|c| c == "pane_close pane=ws-rh:doomed"),
            "{:?}",
            h.calls()
        );
    }

    #[test]
    fn a_released_pane_is_written_onto_a_fresh_read() {
        // The success half, and the reason it is not written from the caller's
        // copy: by the time the surrender runs, up to `SEND_READY_TIMEOUT` of
        // polling has happened and capture persists through a copy of its own,
        // so the caller's `RunState` is not the state to write back. Same rule
        // as `surrender_unrecordable_pane`.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-released");
        let mut p = reaped_phase("plan", "claude", None, Some("sess-abc"));
        p.set_pane("ws-rh:doomed");
        run.phases.push(p);
        run.save().unwrap();
        // A capture that landed on DISK while the waits ran, which the caller's
        // copy knows nothing about. It must survive the release.
        let mut mid = RunState::load("rh-released").unwrap();
        mid.find_phase_mut("plan").unwrap().tab_id = Some("tab-captured".into());
        mid.save().unwrap();
        let h = FakeHerdr::new();

        surrender_misattributed_pane(&h, &mut run, "plan", "ws-rh:doomed").unwrap();

        let on_disk = RunState::load("rh-released").unwrap();
        let p = on_disk.find_phase("plan").unwrap();
        assert_eq!(p.pane_id(), None, "{p:?}");
        assert!(p.is_reaped(), "{p:?}");
        assert_eq!(
            p.tab_id.as_deref(),
            Some("tab-captured"),
            "a write from the caller's stale copy would have lost this: {p:?}"
        );
        assert!(on_disk.retired_panes.iter().any(|x| x == "ws-rh:doomed"));
        // …and the caller's copy is brought up to date rather than left lying.
        assert!(run.find_phase("plan").unwrap().is_reaped());
    }

    #[test]
    fn two_rehydrates_of_one_run_cannot_both_launch() {
        // Two clicks on ⟳ before the button disables (a reseed can take 30s),
        // two browser tabs, or a retried POST: both processes load their own
        // `RunState`, both see `pane_id == None`, both pass the refusal, both
        // launch. `RunState::save` is whole-file last-write-wins, so one of two
        // genuinely running agents is dropped from `state.json` — an
        // unrecorded live pane, which is the immortal-pane bug arriving by a
        // different road.
        //
        // The lock is what serializes them, and the RE-READ under it is what
        // makes serializing enough: the loser must see what the winner wrote,
        // or it simply launches second.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-concurrent");
        run.phases
            .push(reaped_phase("plan", "claude", None, Some("sess-c")));
        run.save().unwrap();

        // (a) while another process holds the lock, nothing is launched at all.
        let held = acquire_run_lock("rh-concurrent").expect("first holder");
        let h = FakeHerdr::new();
        let err = phase_rehydrate(&h, &mut run, "plan")
            .expect_err("a second rehydrate must not run beside the first");
        assert_eq!(err.kind(), io::ErrorKind::WouldBlock, "{err}");
        assert!(
            h.calls().is_empty(),
            "a refused rehydrate launches nothing: {:?}",
            h.calls()
        );
        drop(held);

        // (b) and once the holder is gone, the loser reads what the winner
        // wrote rather than its own stale copy. Model the winner by writing a
        // live pane to DISK only — the caller's `run` still says the phase is
        // reaped, which is exactly the stale view the race exploits.
        let mut winner = RunState::load("rh-concurrent").unwrap();
        winner.find_phase_mut("plan").unwrap().set_pane("ws-rh:winner");
        winner.save().unwrap();
        assert!(
            run.find_phase("plan").unwrap().is_reaped(),
            "the caller's copy is deliberately stale"
        );

        let err = phase_rehydrate(&h, &mut run, "plan")
            .expect_err("the phase holds a pane on disk, so there is nothing to bring back");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
        assert!(err.to_string().contains("ws-rh:winner"), "{err}");
        assert!(
            h.calls().is_empty(),
            "and still nothing launched: {:?}",
            h.calls()
        );
    }

    #[test]
    fn a_rehydrate_that_cannot_record_its_pane_does_not_leave_it_live() {
        // ⚠️ THE IMMORTAL-PANE BUG, third sighting. `launch_in_pane` succeeded,
        // so an agent is RUNNING in `pane` — and then the save that would have
        // recorded it failed. main's `8173f03` made `drovr cleanup` close only
        // panes it can PROVE are drovr's, so a live tab nothing records reads as
        // the human's: never closed, and it blocks `workspace_close` for the
        // whole run.
        //
        // "Never live-but-unrecorded" is the invariant. When `state.json`
        // cannot be written at all, the only way to keep it is to not leave the
        // pane live.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-unsaveable");
        run.phases
            .push(reaped_phase("plan", "claude", None, Some("sess-u")));
        run.save().unwrap();
        let h = FakeHerdr::new();

        // Make every later write to the run dir fail: tmp+rename cannot create
        // `state.json.tmp` in a directory it may not write.
        //
        // The run lock file is created FIRST, because O_CREAT needs write
        // permission on the directory and the lock is taken before the launch —
        // otherwise this test would model "rehydrate refused up front", which
        // is a different (and much less interesting) failure. An existing file
        // opens fine in a read-only directory, which is also what the real
        // shapes of this failure look like: ENOSPC or EIO on a run dir whose
        // lock file has been there since the first rehydrate.
        use std::os::unix::fs::PermissionsExt;
        let dir = run_dir("rh-unsaveable");
        std::fs::File::create(dir.join("run.lock")).unwrap();
        let before = std::fs::metadata(&dir).unwrap().permissions();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let err = phase_rehydrate(&h, &mut run, "plan");
        std::fs::set_permissions(&dir, before).unwrap();

        let err = err.expect_err("a rehydrate that cannot record its pane has not succeeded");
        assert!(
            err.to_string().contains("could not be recorded"),
            "the operator has to be told the record is what failed: {err}"
        );
        let calls = h.calls();
        let pane = calls
            .iter()
            .find_map(|c| c.strip_prefix("pane_run pane="))
            .map(|c| c.split_whitespace().next().unwrap_or("").to_owned())
            .expect("the launch really happened");
        assert!(
            calls.iter().any(|c| c == &format!("pane_close pane={pane}")),
            "the live pane must be taken back, not left for cleanup to disown: {calls:?}"
        );
        // …and the phase on disk is untouched, so a retry starts from the same
        // place rather than from a half-rehydrated one.
        let on_disk = RunState::load("rh-unsaveable").unwrap();
        let p = on_disk.find_phase("plan").unwrap();
        assert!(p.is_reaped(), "still reaped: {p:?}");
        assert_eq!(p.pane_id(), None, "no pane recorded: {p:?}");
    }

    #[test]
    fn a_failed_rehydrate_leaves_the_phases_completion_evidence_intact() {
        // ⚠️ THE ONE THAT DESTROYS EVIDENCE. Task 1 established that the
        // `<phase>.done` MARKER is the proof a phase completed, and that a
        // stale `Done` status is explicitly NOT accepted instead. So a rehydrate
        // that swept the marker (or minted a new pass, which makes the old
        // marker's token mismatch) and THEN failed to relaunch left a phase
        // neither complete-provable nor running: `phase wait` blocks forever and
        // the only way out is hand-editing the run dir.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-keeps-evidence");
        let mut p = reaped_phase("plan", "claude", None, Some("sess-e"));
        p.pass = crate::run::PassToken::new("the-pass-that-finished-it".into());
        run.phases.push(p);
        run.save().unwrap();
        std::fs::create_dir_all(run_dir("rh-keeps-evidence")).unwrap();
        std::fs::write(
            done_marker("rh-keeps-evidence", "plan"),
            "the-pass-that-finished-it",
        )
        .unwrap();
        let h = FakeHerdr::new();
        h.fail_pane_run();

        assert!(phase_rehydrate(&h, &mut run, "plan").is_err());

        // The evidence, and the token that makes it readable, both survive.
        assert!(
            done_marker("rh-keeps-evidence", "plan").exists(),
            "a failed rehydrate must not destroy the completion marker"
        );
        let on_disk = RunState::load("rh-keeps-evidence").unwrap();
        let phase = on_disk.find_phase("plan").unwrap();
        assert_eq!(
            phase.pass.as_ref().map(|t| t.as_str()),
            Some("the-pass-that-finished-it"),
            "the marker is only evidence while the phase still holds its token"
        );
        // …and nothing else moved either: a retry starts from the same place.
        assert!(phase.is_reaped());
        assert_eq!(phase.pane_id(), None);
        assert!(phase.resume_target().is_some(), "the session survives too");
        assert_eq!(phase.status, PhaseStatus::Done);
    }

    #[test]
    fn a_failed_reseed_launch_does_not_strand_its_tab_either() {
        // The launch-failure path was only covered on the RESUMING branch. The
        // reseed branch reaches `tab_create` through different code (it has
        // already replaced the agent record), and it must dispose of its orphan
        // the same way — a pane drovr opened and never recorded is one cleanup
        // protects as the human's, forever, while it blocks `workspace_close`.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-reseed-failed");
        run.phases
            .push(reaped_phase("plan", "claude", None, None)); // no session → reseed
        run.save().unwrap();
        let h = FakeHerdr::new();
        h.fail_pane_run();

        assert!(phase_rehydrate(&h, &mut run, "plan").is_err());

        let tab = h
            .calls()
            .into_iter()
            .find(|c| c.contains("tab_create"))
            .expect("it got as far as making a tab");
        let pane = tab.rsplit("-> ").next().unwrap().to_owned();
        let on_disk = RunState::load("rh-reseed-failed").unwrap();
        assert!(
            on_disk.retired_panes.contains(&pane),
            "the orphan must stay recorded as drovr's: {:?}",
            on_disk.retired_panes
        );
        assert!(on_disk.find_phase("plan").unwrap().is_reaped(), "still reaped");
        assert_eq!(on_disk.find_phase("plan").unwrap().pane_id(), None);
    }

    #[test]
    fn rehydrate_refuses_a_phase_whose_launch_never_completed() {
        // `phase_start` persists `Running` + the new pass BEFORE it launches, so
        // a launch that fails leaves a phase that LOOKS started — `has_run()` is
        // true — but never had an agent in it: no record, and no `handoff_doc`,
        // because both are written only after the launch succeeds. Relaunching
        // it here would be `phase start` under a name that promises recovery,
        // and would silently drop the seed the original call was given.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-failed-start");
        let mut p = Phase::new("plan");
        p.status = PhaseStatus::Running; // what phase_start persisted…
        run.phases.push(p); // …and then the launch failed.
        run.save().unwrap();
        let h = FakeHerdr::new();

        assert!(run.find_phase("plan").unwrap().has_run(), "the trap: it looks started");
        let err = phase_rehydrate(&h, &mut run, "plan")
            .expect_err("a phase with no agent on record is not rehydratable");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
        assert!(
            err.to_string().contains("drovr phase start 'rh-failed-start' 'plan'"),
            "must name the command that CAN carry the seed: {err}"
        );
        assert!(
            err.to_string().contains("seed"),
            "must say the seed is what a rehydrate cannot recover: {err}"
        );
        assert!(h.calls().is_empty(), "nothing launched: {:?}", h.calls());

        // A phase REAPED by a build older than the agent record still qualifies:
        // reaping only ever touches a phase that held a pane, so it demonstrably
        // ran, and refusing it would strand exactly the case rehydrate exists for.
        run.phases[0].set_pane("legacy-pane");
        run.phases[0].mark_reaped();
        run.phases[0].clear_pane_agent_for_test();
        // `run.agent` is the ONLY backend such a phase can be relaunched under —
        // there is no agent record to read one from. Set it to something that is
        // NOT the built-in default, or "falls back to run.agent" and "hardcodes
        // claude" are the same assertion.
        run.agent = Some("cursor".into());
        // Saved: `phase_rehydrate` re-reads `state.json` under its lock, so an
        // in-memory-only change is not a change at all.
        run.save().unwrap();
        assert!(phase_rehydrate(&h, &mut run, "plan").is_ok());
        let launch = pane_run_call(&h);
        assert!(
            launch.contains("agent --workspace"),
            "the run's backend decides, not a hardcoded claude: {launch}"
        );
        assert!(
            !launch.contains("claude"),
            "no trace of the default backend: {launch}"
        );
    }

    #[test]
    fn rehydrate_of_an_unknown_phase_does_not_append_it() {
        // `phase_start` appends any name it is given; rehydrate must not, or a
        // typo (or an HTTP caller) silently creates a phase.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-unknown");
        run.phases.push(reaped_phase("plan", "claude", None, Some("s1")));
        run.save().unwrap();
        let h = FakeHerdr::new();

        let err = phase_rehydrate(&h, &mut run, "nope").expect_err("unknown phase must refuse");
        assert_eq!(err.kind(), io::ErrorKind::NotFound, "{err}");
        assert_eq!(run.phases.len(), 1, "nothing appended");
        assert!(run.find_phase("nope").is_none());
        assert!(h.calls().is_empty(), "{:?}", h.calls());
    }

    #[test]
    fn rehydrate_clears_the_stale_marker_and_mints_a_new_pass() {
        // The old pass's marker would otherwise complete the next `phase wait`
        // instantly, off work that predates this rehydrate.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-marker");
        let mut p = reaped_phase("plan", "claude", None, Some("sess-m"));
        p.pass = crate::run::PassToken::new("old-pass".into());
        run.phases.push(p);
        run.save().unwrap();
        std::fs::create_dir_all(run_dir("rh-marker")).unwrap();
        std::fs::write(done_marker("rh-marker", "plan"), "old-pass").unwrap();
        let h = FakeHerdr::new();
        // herdr reports the resumed conversation back — a resume is not
        // confirmed on readiness alone.
        script_resumed_session(&h, "sess-m", "claude", 8);

        phase_rehydrate(&h, &mut run, "plan").unwrap();

        assert!(
            !done_marker("rh-marker", "plan").exists(),
            "on SUCCESS the previous pass's marker is gone — it describes work \
             the rehydrated agent is about to redo"
        );
        let pass = run.find_phase("plan").unwrap().pass.clone().unwrap();
        assert_ne!(pass.as_str(), "old-pass", "a new agent is a new pass");
        let launch = pane_run_call(&h);
        assert!(
            launch.contains(&format!("{PASS_ENV}='{}'", pass.as_str())),
            "the agent carries the pass it was launched under: {launch}"
        );
    }

    #[test]
    fn a_rehydrate_whose_launch_fails_does_not_strand_its_tab() {
        // Same class as `phase_start`'s: a pane drovr opened and never recorded
        // is one `drovr cleanup` protects as the human's, forever — and it
        // blocks `workspace_close` for the whole run.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-failed-launch");
        run.phases
            .push(reaped_phase("plan", "claude", None, Some("sess-f")));
        run.save().unwrap();
        let h = FakeHerdr::new();
        h.fail_pane_run();

        assert!(phase_rehydrate(&h, &mut run, "plan").is_err());

        let tab = h
            .calls()
            .into_iter()
            .find(|c| c.contains("tab_create"))
            .unwrap();
        let pane = tab.rsplit("-> ").next().unwrap().to_owned();
        let on_disk = RunState::load("rh-failed-launch").unwrap();
        assert!(
            on_disk.retired_panes.contains(&pane),
            "the orphan must stay recorded as drovr's: {:?}",
            on_disk.retired_panes
        );
        // The phase itself is untouched: still reaped, still resumable.
        assert!(on_disk.find_phase("plan").unwrap().is_reaped());
        assert!(on_disk.find_phase("plan").unwrap().resume_target().is_some());
    }

    #[test]
    fn rehydrate_never_reaps_or_closes_anything() {
        // ⭐ KEPT DELIBERATELY once reaping landed, with its reason rewritten.
        //
        // It was a scope guard — "nothing closes panes yet" — and
        // that reading expired the moment `phase_reap` existed. What it
        // ASSERTS did not: reaping is triggered by supersession, and bringing a
        // phase back is the opposite of superseding it, so a rehydrate that
        // works must still close nothing. Deleting it would have removed the
        // only thing standing between that rule and a future edit that reaps
        // "while we are in here anyway".
        //
        // The panes this file does close are all one of two things, neither of
        // them this path: error recovery on a half-completed operation (a
        // launch that failed — `discard_unlaunched_pane`; a pane that could not
        // be recorded — `surrender_unrecordable_pane`; one the phase's record
        // cannot account for — `surrender_misattributed_pane`), or a reap,
        // which has its own triggers and its own tests. Each has its own test;
        // this one pins the success path.
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = rehydrate_run("rh-no-close");
        run.phases
            .push(reaped_phase("plan", "claude", None, Some("sess-n")));
        run.save().unwrap();
        let h = FakeHerdr::new();
        // herdr reports the resumed conversation back — a resume is not
        // confirmed on readiness alone.
        script_resumed_session(&h, "sess-n", "claude", 8);

        phase_rehydrate(&h, &mut run, "plan").unwrap();
        let calls = h.calls();
        assert!(
            !calls.iter().any(|c| c.contains("tab_close")),
            "{calls:?}"
        );
        assert!(
            !calls.iter().any(|c| c.contains("pane_close")),
            "{calls:?}"
        );
    }
}

/// Reaping — the supersession trigger, `drovr phase reap`, and the three things
/// a pane's standing can be.
#[cfg(test)]
mod reap_tests {
    use super::*;
    use crate::herdr::{FakeHerdr, SessionId};
    use crate::test_util::ENV_LOCK;


    /// A run set up the way reaping meets one: its own data dir, its own config
    /// dir (so `reap_finished_panes` is the built-in default and not whatever
    /// the developer running the tests has configured), and a root shell that is
    /// nobody's phase.
    fn reap_run(name: &str) -> (RunState, tempfile::TempDir) {
        // Caller must hold ENV_LOCK.
        let cfg_home = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", format!("/tmp/drovr-reap-test-{name}"));
            std::env::set_var("XDG_CONFIG_HOME", cfg_home.path());
            std::env::remove_var(PASS_ENV);
            std::env::remove_var("CLAUDE_CONFIG_DIR");
        }
        let _ = std::fs::remove_dir_all(run_dir(name));
        let run = RunState {
            name: name.to_owned(),
            task: "test task".into(),
            agent: Some("claude".into()),
            phases: vec![],
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: Some("ws-rp".into()),
            root_pane: Some("ws-rp:root".into()),
            project_dir: "/tmp/drovr-proj-test".into(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
        };
        (run, cfg_home)
    }

    /// A finished phase still holding its pane — what reaping exists to find.
    fn finished_phase(name: &str, pane: &str) -> Phase {
        let mut p = Phase::new(name);
        p.status = PhaseStatus::Done;
        p.set_pane(pane);
        p.record_launch("claude", None);
        p
    }

    fn write_config(cfg_home: &tempfile::TempDir, body: &str) {
        let path = cfg_home.path().join("drovr/config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn closed_panes(h: &FakeHerdr) -> Vec<String> {
        h.calls()
            .iter()
            .filter_map(|c| c.strip_prefix("pane_close pane=").map(str::to_owned))
            .collect()
    }

    /// The supersession trigger, and the three phases it must leave alone.
    #[test]
    fn a_launch_reaps_the_finished_phases_it_supersedes_and_nothing_else() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("reap-supersede");
        run.phases.push(finished_phase("brainstorm", "ws-rp:p1"));
        // Failed keeps its pane: that pane is what a human attaches to in order
        // to find out what went wrong.
        let mut failed = finished_phase("plan", "ws-rp:p2");
        failed.status = PhaseStatus::Failed;
        run.phases.push(failed);
        // Running keeps its pane for the obvious reason.
        let mut running = finished_phase("spec", "ws-rp:p3");
        running.status = PhaseStatus::Running;
        run.phases.push(running);
        run.save().unwrap();
        let h = FakeHerdr::new();

        phase_start(&h, &mut run, "implement", None).unwrap();

        assert_eq!(
            closed_panes(&h),
            vec!["ws-rp:p1".to_string()],
            "only the finished phase's pane is closed — not Failed, not Running, \
             and never the root shell: {:?}",
            h.calls()
        );
        // The phase is released, and its STATUS is untouched: reaping says
        // something about the pane, not about whether the work was done.
        let on_disk = RunState::load("reap-supersede").unwrap();
        let reaped = on_disk.find_phase("brainstorm").unwrap();
        assert!(reaped.is_reaped());
        assert_eq!(reaped.pane_id(), None);
        assert_eq!(reaped.status, PhaseStatus::Done);
        assert!(
            on_disk.retired_panes.iter().any(|p| p == "ws-rp:p1"),
            "the pane must stay provably drovr's for `drovr cleanup`: {:?}",
            on_disk.retired_panes
        );
        // And the caller's copy agrees with disk, so a save after this does not
        // resurrect the pane it just closed.
        assert!(run.find_phase("brainstorm").unwrap().is_reaped());
    }

    /// The trigger is AFTER the launch, and only a launch that worked is
    /// evidence the run has moved past anything.
    #[test]
    fn a_launch_that_fails_reaps_nothing() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("reap-launch-failed");
        run.phases.push(finished_phase("brainstorm", "ws-rp:p1"));
        run.save().unwrap();
        let h = FakeHerdr::new();
        h.fail_pane_run();

        phase_start(&h, &mut run, "implement", None)
            .expect_err("precondition: the launch failed");

        assert!(
            !closed_panes(&h).iter().any(|p| p == "ws-rp:p1"),
            "a phase that could not be started supersedes nothing: {:?}",
            h.calls()
        );
        assert!(!RunState::load("reap-launch-failed").unwrap()
            .find_phase("brainstorm")
            .unwrap()
            .is_reaped());
    }

    /// Re-entering a phase (`phase start` on one that is already `Done`) must
    /// not close the pane the launch is about to reuse.
    #[test]
    fn a_launch_never_reaps_the_phase_it_is_re_entering() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("reap-reentry");
        run.phases.push(finished_phase("implement", "ws-rp:p1"));
        run.save().unwrap();
        let h = FakeHerdr::new();

        phase_start(&h, &mut run, "implement", None).unwrap();

        assert!(
            closed_panes(&h).is_empty(),
            "the re-entered phase keeps the pane it was just relaunched into: {:?}",
            h.calls()
        );
        assert_eq!(
            run.find_phase("implement").unwrap().pane_id(),
            Some("ws-rp:p1")
        );
    }

    /// The opt-out. Everything else about the launch is unchanged.
    #[test]
    fn reaping_is_off_when_the_config_turns_it_off() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, cfg) = reap_run("reap-opt-out");
        write_config(&cfg, "reap_finished_panes = false\n");
        run.phases.push(finished_phase("brainstorm", "ws-rp:p1"));
        run.save().unwrap();
        let h = FakeHerdr::new();

        phase_start(&h, &mut run, "implement", None).unwrap();

        assert!(
            closed_panes(&h).is_empty(),
            "reaping off means no pane is closed: {:?}",
            h.calls()
        );
        let on_disk = RunState::load("reap-opt-out").unwrap();
        assert_eq!(
            on_disk.find_phase("brainstorm").unwrap().pane_id(),
            Some("ws-rp:p1"),
            "and the phase keeps its pane until `drovr cleanup`"
        );
        // But the explicit command still works — it is an instruction, not a
        // policy.
        assert!(matches!(
            phase_reap(&h, &mut run, "brainstorm").unwrap(),
            ReapOutcome::Closed { .. }
        ));
    }

    /// ⭐ THE REQUIRED FAILURE TEST. A close that does not happen must leave the
    /// phase exactly as it was — because a pane that is still there and no
    /// longer recorded is IMMORTAL: `drovr cleanup` closes only panes it can
    /// prove are drovr's (main's `8173f03`), so an unrecorded one reads as the
    /// human's, is never closed, and blocks `workspace_close` for the whole run.
    #[test]
    fn a_pane_that_cannot_be_closed_leaves_its_phase_exactly_as_it_was() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("reap-close-fails");
        run.phases.push(finished_phase("brainstorm", "ws-rp:p1"));
        run.save().unwrap();
        let h = FakeHerdr::new();
        h.fail_pane_close();

        let outcome = phase_reap(&h, &mut run, "brainstorm")
            .expect("a failed close is best-effort, never an error");
        assert!(
            matches!(
                outcome,
                ReapOutcome::Kept {
                    why: PaneKept::CloseFailed(_),
                    ..
                }
            ),
            "{outcome:?}"
        );

        for state in [&run, &RunState::load("reap-close-fails").unwrap()] {
            let p = state.find_phase("brainstorm").unwrap();
            assert_eq!(p.status, PhaseStatus::Done, "status untouched");
            assert!(!p.is_reaped(), "reaped stays false");
            assert_eq!(
                p.pane_id(),
                Some("ws-rp:p1"),
                "⚠️ the REGISTRATION is what keeps the pane inside \
                 `drovr_pane_ids`, so cleanup can still prove it is drovr's"
            );
        }
    }

    /// And the same failure inside the automatic trigger: the phase that just
    /// started is unaffected, and `phase_start` still succeeds.
    #[test]
    fn a_reap_that_fails_never_fails_the_phase_that_triggered_it() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("reap-best-effort");
        run.phases.push(finished_phase("brainstorm", "ws-rp:p1"));
        run.save().unwrap();
        let h = FakeHerdr::new();
        h.fail_pane_close();

        phase_start(&h, &mut run, "implement", None)
            .expect("a reap that cannot close a pane must not fail the launch");

        let on_disk = RunState::load("reap-best-effort").unwrap();
        assert_eq!(
            on_disk.find_phase("implement").unwrap().status,
            PhaseStatus::Running
        );
        assert_eq!(
            on_disk.find_phase("brainstorm").unwrap().pane_id(),
            Some("ws-rp:p1")
        );
    }

    /// Closing a pane makes herdr reassign focus. drovr captures it first and
    /// puts it back, the same way `launch_in_pane` does — bookkeeping must not
    /// move the user's view.
    #[test]
    fn a_reap_restores_the_focus_its_close_disturbed() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("reap-focus");
        run.phases.push(finished_phase("brainstorm", "ws-rp:p1"));
        run.save().unwrap();
        let h = FakeHerdr::new();

        phase_reap(&h, &mut run, "brainstorm").unwrap();

        let calls = h.calls();
        let focused = calls.iter().position(|c| c == "focused_workspace");
        let close = calls
            .iter()
            .position(|c| c == "pane_close pane=ws-rp:p1")
            .expect("the pane was closed");
        let restore = calls.iter().position(|c| c == "workspace_focus id=ws-focused");
        assert!(
            focused.is_some_and(|f| f < close) && restore.is_some_and(|r| r > close),
            "focus must be captured before the close and restored after it: {calls:?}"
        );
    }

    /// The pane is polled BEFORE it is closed, and the poll is the capturing one
    /// — this is the last time anything will ever look at it. herdr reports
    /// `agent_session` only while the agent is alive, so an id not banked here
    /// is one a rehydrate will never have, and reaping without rehydrate is a
    /// downgrade.
    #[test]
    fn a_reap_banks_the_session_before_it_closes_the_pane() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("reap-capture");
        run.phases.push(finished_phase("brainstorm", "ws-rp:p1"));
        run.save().unwrap();
        let h = FakeHerdr::new();

        phase_reap(&h, &mut run, "brainstorm").unwrap();

        let on_disk = RunState::load("reap-capture").unwrap();
        let p = on_disk.find_phase("brainstorm").unwrap();
        assert_eq!(
            p.pane_agent().and_then(|a| a.session()).map(SessionId::as_str),
            Some(FakeHerdr::session_value_for("ws-rp:p1").as_str()),
            "the session must be captured on the way past, or the phase is \
             reaped and unrehydratable"
        );
        // And it really is rehydratable now — the point of banking it.
        assert_eq!(on_disk.rehydratable("brainstorm"), Ok(()));

        let calls = h.calls();
        let poll = calls
            .iter()
            .position(|c| c.starts_with("pane_info pane=ws-rp:p1"))
            .expect("the pane was polled");
        let close = calls
            .iter()
            .position(|c| c == "pane_close pane=ws-rp:p1")
            .expect("the pane was closed");
        assert!(poll < close, "poll before close: {calls:?}");
    }

    /// Idempotent: the second reap of one phase emits no close at all.
    #[test]
    fn reaping_a_phase_twice_closes_one_pane() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("reap-twice");
        run.phases.push(finished_phase("brainstorm", "ws-rp:p1"));
        run.save().unwrap();
        let h = FakeHerdr::new();

        assert!(matches!(
            phase_reap(&h, &mut run, "brainstorm").unwrap(),
            ReapOutcome::Closed { .. }
        ));
        let after_first = closed_panes(&h).len();
        assert_eq!(
            phase_reap(&h, &mut run, "brainstorm").unwrap(),
            ReapOutcome::NothingToReap
        );
        assert_eq!(
            closed_panes(&h).len(),
            after_first,
            "a phase that holds no pane has nothing to close: {:?}",
            h.calls()
        );
    }

    /// ⭐ The stuck `HoldsPane` repair, which is the whole reason
    /// `drovr phase reap` is a command and not only a trigger.
    ///
    /// Three routes lead to a phase that records a pane herdr no longer has: a
    /// `NeverReady` resume whose agent has since exited, a `ResumeUnobserved`
    /// one, and a pane herdr simply lost. All three used to leave the operator
    /// hand-editing `pane_id` out of `state.json`, because nothing else cleared
    /// it — `rehydratable` answered `HoldsPane` forever.
    #[test]
    fn a_phase_whose_pane_herdr_has_lost_is_released_by_a_reap() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("reap-lost-pane");
        run.phases.push(finished_phase("plan", "ws-rp:p1"));
        run.save().unwrap();
        assert_eq!(
            run.rehydratable("plan"),
            Err(NotRehydratable::HoldsPane("ws-rp:p1".into())),
            "precondition: the registration is what refuses the rehydrate"
        );

        let h = FakeHerdr::new();
        // herdr cannot read the pane AND answers that it does not exist — the
        // only combination that proves it is gone.
        h.fail_pane_info();
        h.kill_pane("ws-rp:p1");

        assert_eq!(
            phase_reap(&h, &mut run, "plan").unwrap(),
            ReapOutcome::Cleared {
                pane: "ws-rp:p1".into()
            }
        );
        assert!(
            closed_panes(&h).is_empty(),
            "there was nothing to close: {:?}",
            h.calls()
        );

        let on_disk = RunState::load("reap-lost-pane").unwrap();
        assert_eq!(on_disk.rehydratable("plan"), Ok(()), "and the refusal is gone");
        assert!(
            on_disk.retired_panes.iter().any(|p| p == "ws-rp:p1"),
            "still recorded as drovr's, in case herdr was wrong"
        );
    }

    /// The other half of that classification, and the one a `bool` would have
    /// got wrong: herdr could not be READ. Nothing was established, so nothing
    /// is destroyed and nothing is dropped — dropping the registration here
    /// strands a pane that may be perfectly alive.
    #[test]
    fn a_reap_that_cannot_reach_herdr_changes_nothing() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("reap-unreadable");
        run.phases.push(finished_phase("plan", "ws-rp:p1"));
        run.save().unwrap();
        let h = FakeHerdr::new();
        // The poll fails, and `pane_exists` is biased toward "alive": only an
        // explicit `pane_not_found` proves death, and an unreachable daemon
        // does not give one.
        h.fail_pane_info();

        assert_eq!(
            phase_reap(&h, &mut run, "plan").unwrap(),
            ReapOutcome::Kept {
                pane: "ws-rp:p1".into(),
                why: PaneKept::Unreadable,
            }
        );
        assert!(closed_panes(&h).is_empty(), "{:?}", h.calls());
        let on_disk = RunState::load("reap-unreadable").unwrap();
        let p = on_disk.find_phase("plan").unwrap();
        assert!(!p.is_reaped());
        assert_eq!(p.pane_id(), Some("ws-rp:p1"));
    }

    /// Never the pane that anchors the workspace. herdr destroys a workspace
    /// when its last pane closes, so this one takes the run with it.
    #[test]
    fn a_reap_refuses_the_workspaces_root_shell() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("reap-root");
        // Only a `state.json` from the build where the first phase claimed the
        // root pane looks like this. Such a run still loads and still works.
        run.phases.push(finished_phase("plan", "ws-rp:root"));
        run.save().unwrap();
        let h = FakeHerdr::new();

        let err = phase_reap(&h, &mut run, "plan").expect_err("the root shell is never reapable");
        assert!(err.to_string().contains("root shell"), "{err}");
        assert!(closed_panes(&h).is_empty(), "{:?}", h.calls());
        assert_eq!(
            RunState::load("reap-root").unwrap()
                .find_phase("plan")
                .unwrap()
                .pane_id(),
            Some("ws-rp:root")
        );
    }

    /// A reap decided on the caller's copy is a reap of whatever pane that copy
    /// remembers. The driver holds one `RunState` across a whole run, and the
    /// review server rehydrates from another process meanwhile.
    #[test]
    fn a_reap_reads_the_run_from_disk_before_it_decides() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("reap-stale-copy");
        run.phases.push(finished_phase("plan", "ws-rp:old"));
        run.save().unwrap();

        // Another process moved the phase onto a different pane (a rehydrate).
        let mut other = RunState::load("reap-stale-copy").unwrap();
        other.find_phase_mut("plan").unwrap().set_pane("ws-rp:new");
        other.save().unwrap();
        assert_eq!(
            run.find_phase("plan").unwrap().pane_id(),
            Some("ws-rp:old"),
            "precondition: the caller's copy is stale"
        );

        let h = FakeHerdr::new();
        phase_reap(&h, &mut run, "plan").unwrap();

        assert_eq!(
            closed_panes(&h),
            vec!["ws-rp:new".to_string()],
            "the pane on disk is the one that gets closed, never the one in hand"
        );
    }

    /// A release that cannot be saved after the pane is already gone is the ONE
    /// failure a reap raises — and the message must name the command that
    /// clears it rather than an edit to `state.json`, because that command now
    /// exists.
    #[test]
    fn a_reap_that_cannot_record_itself_says_what_actually_clears_it() {
        use std::os::unix::fs::PermissionsExt;
        struct RestorePerms(PathBuf, std::fs::Permissions);
        impl Drop for RestorePerms {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, self.1.clone());
            }
        }

        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("reap-unsaveable");
        run.phases.push(finished_phase("plan", "ws-rp:p1"));
        run.save().unwrap();
        let h = FakeHerdr::new();

        // The lock file is created first: `O_CREAT` needs write permission on
        // the directory, and the lock is taken before anything else — otherwise
        // this would model "the reap was refused up front", a different failure.
        let dir = run_dir("reap-unsaveable");
        std::fs::File::create(dir.join("run.lock")).unwrap();
        let before = std::fs::metadata(&dir).unwrap().permissions();
        let _restore = RestorePerms(dir.clone(), before.clone());
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let root = std::fs::write(dir.join(".probe"), b"").is_ok();
        let res = phase_reap(&h, &mut run, "plan");
        std::fs::set_permissions(&dir, before).unwrap();

        if root {
            return;
        }
        let err = res.expect_err("a reap that cannot record itself has not succeeded");
        let msg = err.to_string();
        assert!(msg.contains("ws-rp:p1"), "the pane must be named: {msg}");
        assert!(
            msg.contains("drovr phase reap"),
            "the remedy is the command, not a hand-edit: {msg}"
        );
        assert!(
            !msg.contains("pane_id") && !msg.contains("state.json"),
            "the hand-edit instruction is retired: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // The retired-pane sweep
    // -----------------------------------------------------------------------

    /// The leak this sweep exists to close, end to end.
    ///
    /// `code_review_run` replaces a reviewer by retiring its pane and dropping
    /// the registration. Nothing then closed that pane: `phase_reap` works per
    /// phase and no phase points at it any more, so it survived every trigger
    /// and waited for `drovr cleanup`. That is precisely the accumulation
    /// reaping exists to stop.
    #[test]
    fn a_retired_pane_is_closed_by_the_sweep() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("sweep-closes");
        run.retire_pane("ws-rp:p9");
        run.save().unwrap();
        let h = FakeHerdr::new();

        reap_retired(&h, &mut run);

        assert_eq!(closed_panes(&h), vec!["ws-rp:p9".to_string()]);
        for state in [&run, &RunState::load("sweep-closes").unwrap()] {
            assert!(
                state.retired_panes.is_empty(),
                "a pane that is gone is not a pane cleanup has to be told about: {:?}",
                state.retired_panes
            );
        }
    }

    /// ⭐ THE REQUIRED FAILURE TEST, for the sweep. A close that does not happen
    /// leaves the retirement exactly where it was — it is the ONLY record that
    /// the pane is drovr's, so forgetting it while the pane is still standing
    /// makes it immortal: `drovr cleanup` would read it as the human's, never
    /// close it, and refuse `workspace_close` for the whole run.
    #[test]
    fn a_retired_pane_that_cannot_be_closed_stays_recorded() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("sweep-close-fails");
        run.retire_pane("ws-rp:p9");
        run.save().unwrap();
        let h = FakeHerdr::new();
        h.fail_pane_close();

        reap_retired(&h, &mut run);

        assert_eq!(
            closed_panes(&h),
            vec!["ws-rp:p9".to_string()],
            "it was attempted: {:?}",
            h.calls()
        );
        for state in [&run, &RunState::load("sweep-close-fails").unwrap()] {
            assert_eq!(
                state.retired_panes,
                vec!["ws-rp:p9".to_string()],
                "⚠️ the retirement is what keeps the pane inside `drovr_pane_ids`"
            );
        }
    }

    /// Idempotent: the second sweep emits no close at all. The FIRST sweep is
    /// what makes that true — it forgets the pane it closed, so there is
    /// nothing left to probe, let alone close.
    #[test]
    fn sweeping_twice_closes_one_pane() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("sweep-twice");
        run.retire_pane("ws-rp:p9");
        run.save().unwrap();
        let h = FakeHerdr::new();

        reap_retired(&h, &mut run);
        let after_first = closed_panes(&h);
        assert_eq!(after_first, vec!["ws-rp:p9".to_string()]);

        let before = h.calls().len();
        reap_retired(&h, &mut run);

        assert_eq!(
            closed_panes(&h),
            after_first,
            "a forgotten pane is not closed again: {:?}",
            h.calls()
        );
        assert_eq!(
            h.calls().len(),
            before,
            "and it is not even probed again: {:?}",
            h.calls()
        );
    }

    /// Never the pane that anchors the workspace — the same refusal
    /// `RunState::reapable` makes for a phase, made by the same run for the
    /// retirement list. herdr destroys a workspace when its last pane closes.
    #[test]
    fn the_sweep_never_closes_the_workspaces_root_shell() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("sweep-root");
        // Only a `state.json` from the build where the first phase claimed the
        // root pane reaches this: releasing or surrendering that phase retires
        // the pane it was holding, which is the root shell.
        run.retire_pane("ws-rp:root");
        run.save().unwrap();
        let h = FakeHerdr::new();

        reap_retired(&h, &mut run);

        assert!(
            closed_panes(&h).is_empty(),
            "closing it would take the workspace and every phase in it: {:?}",
            h.calls()
        );
        assert_eq!(
            RunState::load("sweep-root").unwrap().retired_panes,
            vec!["ws-rp:root".to_string()],
            "and it is still recorded, because it is still there"
        );
    }

    /// A retirement and a live registration naming the same pane disagree, and
    /// the registration wins: closing it would leave that phase holding a pane
    /// that is gone — the stuck `HoldsPane` this branch spent a task repairing.
    /// It is also the guard against herdr reissuing a closed pane's id.
    #[test]
    fn the_sweep_leaves_a_pane_a_phase_still_records() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("sweep-still-held");
        run.phases.push(finished_phase("implement", "ws-rp:p1"));
        run.retire_pane("ws-rp:p1");
        run.save().unwrap();
        let h = FakeHerdr::new();

        reap_retired(&h, &mut run);

        assert!(
            closed_panes(&h).is_empty(),
            "the phase still points at it: {:?}",
            h.calls()
        );
        let on_disk = RunState::load("sweep-still-held").unwrap();
        assert_eq!(
            on_disk.find_phase("implement").unwrap().pane_id(),
            Some("ws-rp:p1")
        );
        assert_eq!(on_disk.retired_panes, vec!["ws-rp:p1".to_string()]);
    }

    /// herdr has already lost the pane: nothing to close, and the entry is a
    /// claim with no subject. Forget it — the same `PaneStanding::Gone` reading
    /// that clears a phase's registration.
    #[test]
    fn a_retired_pane_herdr_has_lost_is_forgotten_without_a_close() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("sweep-lost");
        run.retire_pane("ws-rp:p9");
        run.save().unwrap();
        let h = FakeHerdr::new();
        h.fail_pane_info();
        h.kill_pane("ws-rp:p9");

        reap_retired(&h, &mut run);

        assert!(
            closed_panes(&h).is_empty(),
            "there was nothing to close: {:?}",
            h.calls()
        );
        assert!(
            RunState::load("sweep-lost").unwrap().retired_panes.is_empty(),
            "a claim on a pane herdr does not have is a claim on whatever wears \
             that id next"
        );
    }

    /// The other half of that classification, and the one a `bool` would get
    /// wrong: herdr could not be READ. Nothing was established, so nothing is
    /// closed and nothing is forgotten — dropping the retirement here strands a
    /// pane that may be perfectly alive.
    #[test]
    fn a_sweep_that_cannot_reach_herdr_forgets_nothing() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("sweep-unreadable");
        run.retire_pane("ws-rp:p9");
        run.save().unwrap();
        let h = FakeHerdr::new();
        // The poll fails, and `pane_exists` is biased toward "alive": only an
        // explicit `pane_not_found` proves death.
        h.fail_pane_info();

        reap_retired(&h, &mut run);

        assert!(closed_panes(&h).is_empty(), "{:?}", h.calls());
        assert_eq!(
            RunState::load("sweep-unreadable").unwrap().retired_panes,
            vec!["ws-rp:p9".to_string()]
        );
    }

    /// Closing a retired pane disturbs focus exactly as closing a phase's does,
    /// and drovr must not move the user's view as a side effect of its own
    /// bookkeeping.
    #[test]
    fn a_sweep_restores_the_focus_its_close_disturbed() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("sweep-focus");
        run.retire_pane("ws-rp:p9");
        run.save().unwrap();
        let h = FakeHerdr::new();

        reap_retired(&h, &mut run);

        let calls = h.calls();
        let focused = calls.iter().position(|c| c == "focused_workspace");
        let close = calls
            .iter()
            .position(|c| c == "pane_close pane=ws-rp:p9")
            .expect("the pane was closed");
        let restore = calls.iter().position(|c| c == "workspace_focus id=ws-focused");
        assert!(
            focused.is_some_and(|f| f < close) && restore.is_some_and(|r| r > close),
            "focus must be captured before the close and restored after it: {calls:?}"
        );
    }

    /// A sweep decided on the caller's copy is a sweep of whatever that copy
    /// remembers. The driver holds one `RunState` across a whole run, and a
    /// panel retires panes from its own stale snapshot.
    #[test]
    fn a_sweep_reads_the_run_from_disk_before_it_decides() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("sweep-stale-copy");
        run.save().unwrap();

        // Another process retired a pane this copy has never heard of.
        let mut other = RunState::load("sweep-stale-copy").unwrap();
        other.retire_pane("ws-rp:p9");
        other.save().unwrap();
        assert!(
            run.retired_panes.is_empty(),
            "precondition: the caller's copy is stale"
        );

        let h = FakeHerdr::new();
        reap_retired(&h, &mut run);

        assert_eq!(
            closed_panes(&h),
            vec!["ws-rp:p9".to_string()],
            "the retirements on disk are the ones that get swept"
        );
    }

    /// The trigger, at the same moment and under the same config gate as the
    /// supersession reap: a launch is the run provably moving on.
    #[test]
    fn a_launch_sweeps_the_runs_retired_panes() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("sweep-on-launch");
        run.retire_pane("ws-rp:p9");
        run.save().unwrap();
        let h = FakeHerdr::new();

        phase_start(&h, &mut run, "implement", None).unwrap();

        assert!(
            closed_panes(&h).iter().any(|p| p == "ws-rp:p9"),
            "the launch superseded nothing, but the debris is still debris: {:?}",
            h.calls()
        );
    }

    /// And it is best-effort in the same way: a sweep that cannot close
    /// anything must not turn a started phase into a failed command.
    #[test]
    fn a_sweep_that_fails_never_fails_the_phase_that_triggered_it() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, _cfg) = reap_run("sweep-best-effort");
        run.retire_pane("ws-rp:p9");
        run.save().unwrap();
        let h = FakeHerdr::new();
        h.fail_pane_close();

        phase_start(&h, &mut run, "implement", None)
            .expect("a sweep that cannot close a pane must not fail the launch");

        let on_disk = RunState::load("sweep-best-effort").unwrap();
        assert_eq!(
            on_disk.find_phase("implement").unwrap().status,
            PhaseStatus::Running
        );
        assert_eq!(on_disk.retired_panes, vec!["ws-rp:p9".to_string()]);
    }

    /// The opt-out covers the sweep too — it is the same policy, and a human who
    /// turned reaping off did not ask for a different set of their panes to
    /// close.
    #[test]
    fn sweeping_is_off_when_the_config_turns_reaping_off() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (mut run, cfg) = reap_run("sweep-opt-out");
        write_config(&cfg, "reap_finished_panes = false\n");
        run.retire_pane("ws-rp:p9");
        run.save().unwrap();
        let h = FakeHerdr::new();

        phase_start(&h, &mut run, "implement", None).unwrap();

        assert!(
            closed_panes(&h).is_empty(),
            "reaping off means no pane is closed: {:?}",
            h.calls()
        );
        assert_eq!(
            RunState::load("sweep-opt-out").unwrap().retired_panes,
            vec!["ws-rp:p9".to_string()]
        );
    }
}
