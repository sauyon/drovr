use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::load_config;
use crate::herdr::{AgentStatus, Herdr, PromptOutcome};
use crate::run::{PassToken, Phase, PhaseStatus, RunState, run_dir};
use crate::shell::shell_single_quote;

/// How often `phase_wait` polls the filesystem for the completion marker, and
/// how often `wait_agent_ready` polls the pane's agent status.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// How long `phase_send` waits for a freshly-spawned agent to reach its composer
/// before delivering the prompt (see `wait_agent_ready`).
const SEND_READY_TIMEOUT: Duration = Duration::from_secs(30);

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
fn launch_in_pane<H: Herdr>(
    h: &H,
    run_name: &str,
    phase: &str,
    pane: &str,
    command: &str,
    pass: &PassToken,
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
    //   * CLAUDE_CONFIG_DIR selects the caller's claude profile so the agent
    //     authenticates as the right account instead of falling back to
    //     ~/.claude. It is a path, not a secret, so inlining it is safe; it is
    //     propagated from drovr's own environment when set (e.g. under a
    //     `claude-prof` profile). Real secrets (API keys) are never inlined.
    // Values are single-quoted so spaces/metacharacters can't break out.
    let mut env_prefix = format!(
        "env DROVR_PHASE={} {PASS_ENV}={}",
        shell_single_quote(&format!("{run_name}/{phase}")),
        shell_single_quote(pass.as_str()),
    );
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        env_prefix.push_str(&format!(" CLAUDE_CONFIG_DIR={}", shell_single_quote(&dir)));
    }
    let full = format!("{env_prefix} {command}");
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
fn require_pane_id(run: &RunState, phase: &str) -> io::Result<String> {
    let p = run.find_phase(phase).ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, format!("phase not found: {phase}"))
    })?;
    p.pane_id.clone().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("phase has no pane_id: {phase}"),
        )
    })
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
        // A name a REVIEWER already answers to is not free, even though
        // `find_phase_idx` (which searches `phases` only) says it is.
        require_name_unclaimed(&run.review_phases, phase, "a reviewer", &run.name)?;
    } else {
        require_phase_name(phase)?;
    }
    if run.project_dir.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "run '{}' has no project_dir (created before this fix); \
                 please recreate the run with `drovr new`",
                run.name
            ),
        ));
    }
    let cwd = run.project_dir.clone();

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
        // Preserving, like every other writer holding a snapshot the human may
        // have archived under: it only rescues `archived`, so it cannot disturb
        // the pass-token write above.
        run.save_preserving_archived()?;
    }
    remove_stale_marker(&run.name, phase)?;

    // Pick the pane this phase's `claude` will run in, WITHOUT splitting a new
    // pane beside an empty shell:
    //   * a restarting phase reuses its own recorded pane;
    //   * the first phase reuses the workspace's root shell pane (taken here so
    //     later phases don't);
    //   * every later phase gets its own fresh tab (whose auto shell pane it
    //     reuses).
    let existing_pane = find_phase_idx(run, phase).and_then(|i| run.phases[i].pane_id.clone());
    // `used_root` defers consuming `run.root_pane` until the launch actually
    // succeeds — if `pane_run` fails, the root pane stays available for a retry
    // instead of being silently forfeited to a fresh tab.
    let mut used_root = false;
    let target_pane = if let Some(pane) = existing_pane {
        pane
    } else if let Some(root) = run.root_pane.clone() {
        used_root = true;
        root
    } else if let Some(ws) = run.workspace.as_deref() {
        h.tab_create(ws, phase, &cwd)?
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "run '{}' has no herdr workspace (creation failed at `drovr new`); \
                 please recreate the run with `drovr new`",
                run.name
            ),
        ));
    };

    // Use the backend captured by `drovr new`, so every phase stays on the
    // caller's agent even when later commands run from a plain shell.
    let cfg = load_config()?;
    let agent = run.agent.as_deref().unwrap_or("claude");
    // No MCP config: the findings channel exists only for read-only reviewers.
    let command = cfg.launch(agent, &cwd, false, None)?;
    launch_in_pane(h, &run.name, phase, &target_pane, &command, &pass)?;
    // The launch succeeded, so this phase has now claimed the root pane (if it
    // used it); clear it so later phases don't try to reuse the same pane.
    if used_root {
        run.root_pane = None;
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
    run.phases[idx].pane_id = Some(target_pane);
    run.phases[idx].pass = Some(pass);
    run.phases[idx].status = PhaseStatus::Running;

    // Panes are never closed mid-run: closing any pane makes herdr reassign
    // focus, disturbing the user. Every pane drovr opens (the root pane, each
    // phase pane, each reviewer pane) is torn down at the end by `drovr cleanup`,
    // once the user confirms — which is why `pane_id` is recorded here: it is how
    // cleanup knows which panes are drovr's and which are the human's.
    //
    // `save_preserving_archived`, not `save`: the caller has held this state since
    // before the pane was launched, and the human may have archived the run from
    // the web UI in between. Writing a stale `archived: false` back would
    // un-archive a run whose workspace is already destroyed.
    run.save_preserving_archived()?;
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
/// Reviewers ALWAYS get a fresh tab in the run workspace — they never consume
/// `run.root_pane` (that belongs to the pipeline) — so a workspace is required;
/// this errors clearly if `run.workspace` is `None`.
///
/// `seed` (if any) is recorded on the phase's `handoff_doc` for the caller to
/// inject via `phase_send`; it is NOT placed on the command line.
pub fn spawn_reviewer<H: Herdr>(
    h: &H,
    run: &mut RunState,
    phase: &str,
    seed: Option<&Path>,
    launch_command: &str,
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
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "run '{}' has no project_dir (created before this fix); \
                 please recreate the run with `drovr new`",
                run.name
            ),
        ));
    }

    // Reviewers can't reuse the pipeline root pane; they need their own tab, which
    // requires a workspace.
    let ws = run.workspace.clone().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "run '{}' has no herdr workspace; cannot spawn a reviewer \
                 (reviewers need their own tab and never reuse the root pane)",
                run.name
            ),
        )
    })?;

    // A fresh tab (with its auto shell pane) in the run workspace — never the root
    // pane. `tab_create` is `--no-focus`; `launch_in_pane` handles focus around the
    // launch itself.
    // NOTE: deliberately does NOT sweep a pre-existing marker for this name, the
    // way `phase_start` does. Reviewer names embed an iteration counter
    // (`next_iter` = max+1 over an append-only `review_phases`), so a fresh
    // reviewer name never has a marker — and `code_review_run`'s own wait loop
    // treats a pre-dropped marker as a legitimate "already finished" signal, which
    // its tests rely on. Sweeping here is a sound hardening once that loop is
    // token-aware; see the task-1 handoff's note for task 6.
    let pane = h.tab_create(&ws, phase, &run.project_dir)?;
    // Reviewers get a pass token too. They never collide with a previous pass
    // (their names embed `iter`, and `next_iter` takes max+1 over an append-only
    // `review_phases`), so this is uniformity rather than a fix — but it means
    // every marker in the run dir is attributable to the launch that produced it.
    let pass = new_pass_token();
    launch_in_pane(h, &run.name, phase, &pane, launch_command, &pass)?;

    // Register the reviewer in `review_phases` only. The seed path rides on
    // handoff_doc for later `phase_send` injection, mirroring `phase_start`.
    let seed_str = seed.map(|p| p.to_string_lossy().into_owned());
    run.review_phases.push(Phase {
        name: phase.to_owned(),
        status: PhaseStatus::Running,
        handoff_doc: seed_str,
        pane_id: Some(pane),
        pass: Some(pass),
        ..Default::default()
    });
    // Preserving, as in `phase_start`: this runs once per angle inside
    // `code_review_run`'s spawn loop, each iteration a herdr round trip, so an
    // Archive click lands here far more easily than the name suggests.
    run.save_preserving_archived()?;
    Ok(())
}

/// What a scan of a handoff found. An empty `Unfilled` list means every section has a
/// body; it must NOT be the value returned when the scan could not see the sections at all,
/// which is why `Unterminated` is a separate variant rather than an empty vec (review round
/// 2: an unclosed fence swallowed every later heading and the gate passed vacuously).
#[derive(Debug, PartialEq, Eq)]
enum HandoffShape {
    /// Sections with no substantive body, in document order. Empty = complete.
    Unfilled(Vec<String>),
    /// A fenced block or HTML comment is still open at end of file, so structure and
    /// content cannot be told apart below it.
    Unterminated(&'static str),
}

/// Scan a handoff for `## ` sections that carry no substantive body.
///
/// A body is any line that is not blank, not inside an HTML comment (the scaffold's own
/// guidance to the author), and not the placeholder. Fenced blocks are tracked so quoted
/// markdown is content rather than structure — but only their INTERIOR counts as a body:
/// an empty pair of fences is not something an agent wrote (round 2, three angles: opening
/// a fence set the flag before any content existed, so `TODO` plus a lone ``` passed).
///
/// Headings must start at column 0. An indented `## ` is an example inside a list or code
/// sample, not a section, and treating it as one let a phantom heading fracture the model
/// and refuse a finished handoff (round 2, error-handling + security).
///
/// `###` and deeper belong to the enclosing `## ` section.
///
/// **Known gap, stated rather than patched:** a body of `TODO: fill this in` or `- TODO`
/// passes. Separating those from prose that legitimately begins with "TODO" needs a
/// heuristic, and a heuristic under an authoritative gate makes the hole look closed.
/// Content left inside `<!-- -->` also does not count — the scaffold tells the author to
/// delete those comments, and an unrendered comment is not a handoff.
fn scan_handoff(contents: &str) -> HandoffShape {
    let mut unfilled = Vec::new();
    let mut current: Option<(String, bool)> = None;
    let mut in_fence: Option<&'static str> = None;
    let mut in_comment = false;

    for line in contents.lines() {
        let trimmed = line.trim();

        if let Some(marker) = in_fence {
            if trimmed.starts_with(marker) {
                in_fence = None;
                continue;
            }
            // INTERIOR lines are substance; the delimiters themselves are not.
            if let Some((_, has_body)) = current.as_mut()
                && !trimmed.is_empty()
            {
                *has_body = true;
            }
            continue;
        }
        if in_comment {
            if trimmed.contains("-->") {
                in_comment = false;
            }
            continue;
        }

        if let Some(marker) = trimmed
            .starts_with("```")
            .then_some("```")
            .or_else(|| trimmed.starts_with("~~~").then_some("~~~"))
        {
            in_fence = Some(marker);
            continue;
        }
        if trimmed.starts_with("<!--") {
            if !trimmed.contains("-->") {
                in_comment = true;
            }
            continue;
        }

        // Column 0 only — see the doc comment.
        if let Some(rest) = line.strip_prefix("## ") {
            if let Some((heading, has_body)) = current.take()
                && !has_body
            {
                unfilled.push(heading);
            }
            current = Some((rest.trim().to_string(), false));
            continue;
        }

        if let Some((_, has_body)) = current.as_mut()
            && !trimmed.is_empty()
            && trimmed != crate::brief::SCAFFOLD_PLACEHOLDER
        {
            *has_body = true;
        }
    }

    // Unterminated structure means everything below it was misread; say so instead of
    // reporting the sections that happened to be visible above it.
    if in_fence.is_some() {
        return HandoffShape::Unterminated("fenced code block");
    }
    if in_comment {
        return HandoffShape::Unterminated("HTML comment");
    }
    if let Some((heading, has_body)) = current
        && !has_body
    {
        unfilled.push(heading);
    }
    HandoffShape::Unfilled(unfilled)
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
    pane_id: &str,
    timeout: Duration,
    poll_interval: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        // `pane_info` is the only poll; narrow it here rather than through a
        // helper, so "the poll failed" cannot be confused with a status.
        let status = h.pane_info(pane_id).and_then(|info| info.agent_status);
        if agent_has_started(status.as_ref()) {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        thread::sleep(poll_interval.min(deadline - now));
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
/// advances (and, once task 6 lands, reaps a pane it just messaged).
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
    if !wait_agent_ready(h, &pane_id, ready_timeout, poll_interval) {
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
            HandoffShape::Unfilled(unfilled) if !unfilled.is_empty() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "phase '{phase}' cannot signal done: {} of its handoff {} {} empty — \
                         {}. Write {} from your own context (nothing else will); if a section \
                         genuinely has nothing, say so in it (\"None.\"). Text left inside \
                         `<!-- -->` does not count — delete those comments as you fill each \
                         section. THEN run `drovr phase done`.",
                        unfilled.len(),
                        handoff.display(),
                        if unfilled.len() == 1 {
                            "section is"
                        } else {
                            "sections are"
                        },
                        unfilled.join(", "),
                        if unfilled.len() == 1 { "it" } else { "them" },
                    ),
                ));
            }
            HandoffShape::Unterminated(what) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "phase '{phase}' cannot signal done: its handoff {} has an unterminated \
                         {what}, so everything below it reads as quoted text and drovr cannot \
                         tell which sections you filled in. Close it, THEN run \
                         `drovr phase done`.",
                        handoff.display()
                    ),
                ));
            }
            HandoffShape::Unfilled(_) => {}
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
    /// comparison; task 1's handoff §5.1 routes that to task 6, together with the
    /// monotonic re-entry counter it needs.
    ///
    /// Deliberately NOT `TimedOut`, which it used to be reported as. The two are
    /// opposite verdicts about the same phase — "another pass took over, and it is
    /// the one to follow now" versus "the agent I am waiting on is not
    /// progressing" — and nothing but log scraping could tell them apart. Task 6
    /// keys pane teardown off this enum, and the pane here belongs to the LIVE
    /// re-entry: a caller must be able to see that without parsing prose.
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
/// would report success for a phase with no agent running. Task 6 tears panes
/// down on this verdict, so that would close a live agent's pane.
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
    let pane_id = run.phases[idx].pane_id.clone();
    let expected_pass = run.phases[idx].pass.clone();
    let marker = done_marker(&run.name, phase);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut mismatch_reported = false;
    let mut read_error_reported = false;
    // Gated like the other two: the marker keeps matching, so this branch is
    // re-entered on every poll until the deadline.
    let mut token_lost_reported = false;
    loop {
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
                // must never be reported — task 6 tears panes down on this verdict,
                // so an unverifiable `Done` closes a live agent's pane.
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
                        // waiting (task 6 records a reap that way) would otherwise
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
                        // saves after waiting (task 6 records the reap this way)
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
        // Proactively catch a blocked pane so the driver is signalled immediately
        // instead of hanging until the wait's full timeout. Only `blocked` short-
        // circuits; every other status keeps waiting for the marker.
        if let Some(pid) = pane_id.as_deref() {
            if h.pane_info(pid).and_then(|info| info.agent_status) == Some(AgentStatus::Blocked) {
                return Ok(PhaseWaitOutcome::Blocked);
            }
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
fn tail_snippet(pane: &str, n: usize) -> String {
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
    let pane_id = run.find_phase(phase)?.pane_id.clone()?;
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
    let Some(pane_id) = run.phases[idx].pane_id.clone() else {
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
            // `drovr new` always creates a workspace + root shell pane; the first
            // phase reuses the root pane, later phases each get their own tab.
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
        assert!(p.pane_id.is_some(), "pane_id must be recorded");
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
            "claude --permission-mode plan --add-dir '/tmp/drovr-proj-test'",
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

    #[test]
    fn first_phase_reuses_root_pane() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("ws-isolation-test", "ws-42");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();

        let calls = h.calls();
        // The first phase reuses the workspace root pane — no tab is created,
        // and no empty shell is left dangling.
        assert!(
            !calls.iter().any(|c| c.contains("tab_create")),
            "first phase must not create a tab: {calls:?}"
        );
        let run_call = calls.iter().find(|c| c.contains("pane_run")).unwrap();
        assert!(
            run_call.contains("pane=ws-42:root"),
            "first phase must run claude in the root pane: {run_call}"
        );
        assert_eq!(run.phases[0].pane_id.as_deref(), Some("ws-42:root"));
        assert!(
            run.root_pane.is_none(),
            "root_pane must be consumed after first use"
        );
    }

    #[test]
    fn later_phase_creates_its_own_tab() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("later-tab-test", "ws-7");

        phase_start(&h, &mut run, "brainstorm", None).unwrap(); // consumes root pane
        phase_start(&h, &mut run, "plan", None).unwrap(); // must get its own tab

        let calls = h.calls();
        let tab_call = calls.iter().find(|c| c.contains("tab_create")).unwrap();
        assert!(
            tab_call.contains("workspace=ws-7"),
            "tab must be in the run workspace: {tab_call}"
        );
        assert!(
            tab_call.contains("label=plan"),
            "tab must be labelled with the phase: {tab_call}"
        );
        // claude runs in the new tab's pane
        let plan_pane = run.phases[1].pane_id.clone().unwrap();
        assert!(
            calls
                .iter()
                .any(|c| c.contains(&format!("pane_run pane={plan_pane}"))),
            "claude must run in the new tab's pane: {calls:?}"
        );
    }

    #[test]
    fn no_workspace_and_no_root_pane_errors() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("no-ws-test");
        run.workspace = None;
        run.root_pane = None;

        let res = phase_start(&h, &mut run, "plan", None);
        assert!(
            res.is_err(),
            "must error when there is no workspace or root pane"
        );
        assert!(res.unwrap_err().to_string().contains("workspace"));
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
    // plain `save` writes the stale `archived: false` back, leaving a run that
    // looks active but whose workspace is gone and cannot be recreated. These
    // drive the real call sites: mutating either back to `save()` fails here.

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
        // Marker present AND a blocked status queued: the marker is checked first,
        // so the phase is Done and the status is never consulted.
        write_handoff(&run, "plan");
        let pass = run.phases[0].pass.clone().unwrap();
        agent_signals_done(&run, "plan", &pass);
        h.push_status(Some("blocked"));

        let outcome = phase_wait(&h, &mut run, "plan", 5000).unwrap();
        assert_eq!(outcome, PhaseWaitOutcome::Done);
        assert_eq!(run.phases[0].status, PhaseStatus::Done);
        assert!(
            !h.calls().iter().any(|c| c.contains("agent_status")),
            "marker must be checked before status: {:?}",
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
        let pane_id = run.phases[0].pane_id.clone().unwrap();
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
        run.phases.push(Phase {
            name: "code".into(),
            status: PhaseStatus::Running,
            ..Default::default()
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
        run.review_phases.push(Phase {
            name: "review:t:1:correctness".into(),
            status: PhaseStatus::Running,
            ..Default::default()
        });
        assert!(phase_done(&run, "nonexistent").is_err());
    }

    #[test]
    fn done_succeeds_for_review_phase() {
        let _lock = ENV_LOCK.lock().unwrap();
        let mut run = make_run("done-review-test");
        // A reviewer phase lives only in `review_phases`, yet must be able to drop
        // its completion marker via `drovr phase done` (which calls phase_done).
        run.review_phases.push(Phase {
            name: "review:t:1:correctness".into(),
            status: PhaseStatus::Running,
            pane_id: Some("rp1".into()),
            ..Default::default()
        });
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
        run.phases.push(Phase {
            name: "plan".into(),
            status: PhaseStatus::Running,
            pane_id: Some("p1".into()),
            ..Default::default()
        });
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
        run.phases.push(Phase {
            name: "plan".into(),
            status: PhaseStatus::Running,
            pane_id: Some("p1".into()),
            ..Default::default()
        });
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
        run.review_phases.push(Phase {
            name: "review:t:1:correctness".into(),
            status: PhaseStatus::Running,
            pane_id: Some("review-pane-9".into()),
            ..Default::default()
        });
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
        let pane_id = run.phases[0].pane_id.as_ref().unwrap();
        assert!(send_call.contains(pane_id.as_str()));
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
        let pane_id = run.phases[0].pane_id.clone().unwrap();
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

    // Panes are never closed mid-run (herdr reassigns focus on any close); every
    // pane drovr opened is reaped at end-of-run by `drovr cleanup`. `phase_start`
    // must therefore never close a pane.
    #[test]
    fn phase_start_never_closes_a_pane() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("no-mid-run-close-test");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();
        phase_start(&h, &mut run, "plan", None).unwrap();

        let calls = h.calls();
        assert!(
            !calls.iter().any(|c| c.contains("pane_close")),
            "phase_start must never close a pane mid-run: {calls:?}"
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
    // agent is still alive in the reused pane (panes are never closed mid-run) and
    // can run `drovr phase done` again at any moment, recreating the marker after
    // the delete. Every test below drives that exact sequence.

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
        run.phases.push(Phase {
            name: "plan".into(),
            status: PhaseStatus::Running,
            pane_id: Some("p1".into()),
            ..Default::default()
        });
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
        // completion that cannot be verified must never be reported: task 6 tears
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
        // waiting — task 6 records the reap exactly that way — would write the
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
        run.phases.push(Phase {
            name: "plan".into(),
            status: PhaseStatus::Running,
            pane_id: Some("p1".into()),
            ..Default::default()
        });
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
        // task 6 tears panes down on this one.
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
        // after waiting cannot write an hour-old snapshot back; task 6 records a
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
        run.phases.push(Phase {
            name: "plan".into(),
            status: PhaseStatus::Running,
            pane_id: Some("p1".into()),
            ..Default::default()
        });
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

    /// A scaffolded handoff whose sections are still `TODO` is not a handoff — it is the
    /// empty form, and the next phase inherits nothing from it. `phase done` must refuse
    /// it for the same reason it refuses an empty file, and must name the sections so the
    /// agent knows what is missing.
    #[test]
    fn phase_done_refuses_a_handoff_left_at_its_scaffold_todos() {
        let _lock = ENV_LOCK.lock().unwrap();
        let mut run = make_run("handoff-todo-test");
        run.phases.push(Phase {
            name: "plan".into(),
            status: PhaseStatus::Running,
            pane_id: Some("p1".into()),
            ..Default::default()
        });
        run.save().unwrap();

        let hp = run_dir(&run.name).join("plan-HANDOFF.md");
        std::fs::create_dir_all(hp.parent().unwrap()).unwrap();
        std::fs::write(
            &hp,
            "## Objective\n\nship the widget\n\n## State\n\nTODO\n\n\
             ## Decisions + rationale\n\nTODO\n",
        )
        .unwrap();

        let err = phase_done(&run, "plan").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("State"), "must name the unfilled section: {msg}");
        assert!(
            msg.contains("Decisions + rationale"),
            "must name every unfilled section, not just the first: {msg}"
        );
        assert!(
            !msg.contains("Objective"),
            "a section that WAS filled must not be reported: {msg}"
        );
        assert!(
            !done_marker(&run.name, "plan").exists(),
            "no marker may be dropped for a refused handoff"
        );
    }

    /// Review round 1, correctness + type-design: deleting the placeholders without
    /// writing anything passed the gate — shape without substance, which is exactly what
    /// the gate exists to stop.
    #[test]
    fn phase_done_refuses_a_handoff_whose_sections_were_emptied_not_filled() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = registered_phase_run("handoff-emptied");
        write_raw_handoff(
            &run,
            "plan",
            "## Objective\n\nship it\n\n## State\n\n## Decisions + rationale\n\n",
        );

        let err = phase_done(&run, "plan").unwrap_err().to_string();
        assert!(err.contains("State"), "an empty section is unfilled: {err}");
        assert!(err.contains("Decisions + rationale"), "{err}");
        assert!(!err.contains("Objective"), "a filled section is not: {err}");
    }

    /// Review round 1, correctness + error-handling: a `TODO` line inside a fenced block,
    /// in a section full of real prose, was refused — the gate calling a finished handoff
    /// empty at the agent's LAST action, with no explanation of how to escape.
    #[test]
    fn a_fenced_todo_is_content_not_a_placeholder() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = registered_phase_run("handoff-fenced-todo");
        write_raw_handoff(
            &run,
            "plan",
            "## Objective\n\nship it\n\n## State\n\nThe upstream stub still reads:\n\n\
             ```rust\nfn f() {\n    TODO\n}\n```\n\nand we left it alone.\n",
        );

        phase_done(&run, "plan").expect("a quoted TODO must not read as an unfilled section");
    }

    /// Review round 1: a `## ` line inside a fence was treated as a heading, so the refusal
    /// could name a section that does not exist while the real one went unmentioned.
    #[test]
    fn a_fenced_heading_does_not_become_a_section() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = registered_phase_run("handoff-fenced-heading");
        write_raw_handoff(
            &run,
            "plan",
            "## Objective\n\nship it\n\n## State\n\nThe template we emit looks like:\n\n\
             ```markdown\n## Phantom\n\nTODO\n```\n\n## Next step\n\n",
        );

        let err = phase_done(&run, "plan").unwrap_err().to_string();
        assert!(
            !err.contains("Phantom"),
            "a heading inside a fence is quoted text, not structure: {err}"
        );
        assert!(
            !err.contains("State"),
            "the enclosing section has a body: {err}"
        );
        assert!(err.contains("Next step"), "the real empty section: {err}");
    }

    /// The scaffold's own guidance comments are not a body — a section holding only them is
    /// still unfilled, including when the comment spans lines.
    #[test]
    fn scaffold_guidance_comments_are_not_a_body() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = registered_phase_run("handoff-comments");
        write_raw_handoff(
            &run,
            "plan",
            "<!-- Scaffolded by drovr.\nDelete these comments as you go. -->\n\n\
             ## Objective\n\n<!-- One or two lines: what this phase was for. -->\n\nTODO\n",
        );

        let err = phase_done(&run, "plan").unwrap_err().to_string();
        assert!(err.contains("Objective"), "{err}");
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

    /// Round 2, three angles: opening a fence set `has_body` before any content existed,
    /// so `TODO` plus a lone ``` — or an empty fence pair — passed the gate.
    #[test]
    fn an_empty_fence_is_not_a_body() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = registered_phase_run("handoff-empty-fence");
        write_raw_handoff(
            &run,
            "plan",
            "## Objective\n\nship it\n\n## State\n\n```\n```\n\n## Next step\n\n```rust\n```\n",
        );

        let err = phase_done(&run, "plan").unwrap_err().to_string();
        assert!(err.contains("State"), "an empty fence pair is not prose: {err}");
        assert!(err.contains("Next step"), "nor is one with an info string: {err}");
    }

    /// Round 2, correctness + security: an unclosed fence swallowed every later heading, so
    /// sections below it never entered the model and the gate passed vacuously.
    #[test]
    fn an_unterminated_fence_is_refused_rather_than_passing_vacuously() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = registered_phase_run("handoff-unclosed-fence");
        write_raw_handoff(
            &run,
            "plan",
            "## Objective\n\nship it\n\n```\nnever closed\n\n## Next step\n\nTODO\n",
        );

        let err = phase_done(&run, "plan").unwrap_err().to_string();
        assert!(
            err.contains("unterminated") && err.contains("fenced code block"),
            "must name the real problem, not the sections it could not see: {err}"
        );
        assert!(err.contains("Close it"), "must say how to escape: {err}");
    }

    /// The same hole via an unclosed HTML comment.
    #[test]
    fn an_unterminated_comment_is_refused() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = registered_phase_run("handoff-unclosed-comment");
        write_raw_handoff(
            &run,
            "plan",
            "## Objective\n\nship it\n\n<!-- note I forgot to close\n\n## Next step\n\nTODO\n",
        );

        let err = phase_done(&run, "plan").unwrap_err().to_string();
        assert!(err.contains("HTML comment"), "{err}");
    }

    /// Round 2, error-handling + security: an INDENTED `## ` — an example inside a list or
    /// sample — was parsed as a real heading, fracturing the section model and refusing a
    /// finished handoff at the agent's last action.
    #[test]
    fn an_indented_heading_is_an_example_not_a_section() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = registered_phase_run("handoff-indented-heading");
        write_raw_handoff(
            &run,
            "plan",
            "## Objective\n\nship it\n\n## State\n\nThe scaffold emits:\n\n    ## Phantom\n\n\
             and we fill it in.\n",
        );

        phase_done(&run, "plan")
            .expect("an indented heading must not fracture a filled section");
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

    /// The check keys on the scaffold's own placeholder — a line that is exactly `TODO` —
    /// so a handoff that merely discusses TODOs in prose still completes.
    #[test]
    fn phase_done_accepts_a_handoff_that_only_mentions_todo_in_prose() {
        let _lock = ENV_LOCK.lock().unwrap();
        let mut run = make_run("handoff-todo-prose");
        run.phases.push(Phase {
            name: "plan".into(),
            status: PhaseStatus::Running,
            pane_id: Some("p1".into()),
            ..Default::default()
        });
        run.save().unwrap();

        let hp = run_dir(&run.name).join("plan-HANDOFF.md");
        std::fs::create_dir_all(hp.parent().unwrap()).unwrap();
        std::fs::write(
            &hp,
            "## Objective\n\nship it\n\n## Open questions\n\n\
             The TODO in `parser.rs:88` is deliberate; leave it. TODO markers in the \
             vendored dir are not ours.\n",
        )
        .unwrap();

        phase_done(&run, "plan").expect("prose mentioning TODO is a filled section");
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
        run.phases.push(Phase {
            name: "plan".into(),
            status: PhaseStatus::Running,
            pane_id: Some("p1".into()),
            ..Default::default()
        });
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
        run.phases.push(Phase {
            status: PhaseStatus::Running,
            pane_id: Some("p1".into()),
            ..Default::default()
        });
        assert_eq!(run.phases[0].name, "");
        assert!(phase_done(&run, "").is_err(), "phase_done must refuse");
        assert!(phase_wait(&h, &mut run, "", 10).is_err(), "phase_wait must refuse");
        assert!(
            phase_send(&h, &mut run, "", "text").is_err(),
            "phase_send must refuse"
        );
        assert!(
            spawn_reviewer(&h, &mut run, "", None, "claude").is_err(),
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

        // First phase reuses the root pane (whose cwd was set at workspace
        // create); a later phase's tab must carry project_dir as its cwd.
        phase_start(&h, &mut run, "brainstorm", None).unwrap();
        phase_start(&h, &mut run, "plan", None).unwrap();

        let calls = h.calls();
        let tab_call = calls.iter().find(|c| c.contains("tab_create")).unwrap();
        assert!(
            tab_call.contains("cwd=/home/user/my-project"),
            "tab_create must use project_dir as cwd, got: {tab_call}"
        );
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

    // -- F2 (agy correctness): a failed launch must NOT consume the root pane, so
    //    a retry can still reuse it rather than forfeiting it to a fresh tab.
    #[test]
    fn first_phase_keeps_root_pane_on_launch_failure() {
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
            "root_pane must be retained when the launch fails"
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
            "claude --permission-mode plan",
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
        assert!(p.pane_id.is_some(), "pane_id must be recorded");

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

    #[test]
    fn spawn_reviewer_always_creates_tab_never_root_pane() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("rev-tab-test", "ws-rt");
        // root_pane is Some — a pipeline phase would reuse it, but a reviewer must NOT.
        assert!(run.root_pane.is_some());

        spawn_reviewer(
            &h,
            &mut run,
            "review:task-1:1:security",
            None,
            "claude --permission-mode plan",
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
        // Root pane untouched — still available for the pipeline.
        assert_eq!(
            run.root_pane.as_deref(),
            Some("ws-rt:root"),
            "reviewer must not consume the pipeline root pane"
        );
        let reviewer_pane = run.review_phases[0].pane_id.clone().unwrap();
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
            "claude --permission-mode plan",
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
    fn spawn_reviewer_errors_without_workspace() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("rev-no-ws-test");
        run.workspace = None;

        let res = spawn_reviewer(
            &h,
            &mut run,
            "review:t:1:correctness",
            None,
            "claude --permission-mode plan",
        );
        assert!(res.is_err(), "must error when the run has no workspace");
        assert!(res.unwrap_err().to_string().contains("workspace"));
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
            "claude --permission-mode plan",
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
        let reviewer_pane = run.review_phases[0].pane_id.clone().unwrap();
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
            "claude --permission-mode plan",
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
    }

    #[test]
    fn spawn_reviewer_empty_project_dir_returns_error() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("rev-empty-proj-test", "ws-e");
        run.project_dir = String::new();

        let result = spawn_reviewer(&h, &mut run, "review:t:1:correctness", None, "claude");
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

        let err =
            spawn_reviewer(&h, &mut run, "review:t$(id):1:correctness", None, "claude").unwrap_err();
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
        )
        .unwrap();

        let calls = h.calls();
        let run_call = calls.iter().find(|c| c.contains("pane_run")).unwrap();
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
        legacy.phases.push(Phase {
            name: "plan".into(),
            status: PhaseStatus::Running,
            pane_id: Some("p1".into()),
            ..Default::default()
        });
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
        run.review_phases.push(Phase {
            name: "review:t:1:correctness".into(),
            status: PhaseStatus::Done,
            pane_id: Some("p9".into()),
            ..Default::default()
        });
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
        run.review_phases.push(Phase {
            name: legacy.into(),
            status: PhaseStatus::Running,
            pane_id: Some("p7".into()),
            ..Default::default()
        });
        run.save().unwrap();

        // The name may no longer be CREATED …
        assert!(
            spawn_reviewer(&h, &mut run, legacy, None, "claude").is_err(),
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
        spawn_reviewer(&h, &mut run, name, None, "claude").unwrap();
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

        let err = spawn_reviewer(&h, &mut run, "plan", None, "claude").unwrap_err();

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

        spawn_reviewer(&h, &mut run, name, None, "claude").unwrap();
        let err = spawn_reviewer(&h, &mut run, name, None, "claude").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(run.review_phases.len(), 1, "no second entry under one name");

        // Drop the stale registration, exactly as main's respawn does — now it
        // spawns.
        run.review_phases.retain(|p| p.name != name);
        spawn_reviewer(&h, &mut run, name, None, "claude")
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
        run.phases.push(Phase {
            name: legacy.into(),
            status: PhaseStatus::Running,
            pane_id: Some("p3".into()),
            ..Default::default()
        });
        run.save().unwrap();

        phase_start(&h, &mut run, legacy, None).expect("re-entry of an existing phase is allowed");
        assert_eq!(run.phases.len(), 1, "re-entry reuses the entry, never appends");
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
