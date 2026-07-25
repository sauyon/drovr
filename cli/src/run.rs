use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::{fs, io};

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
/// Serializes transparently as a JSON string, so `state.json` is unchanged.
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Debug, Default)]
pub struct PassToken(String);

impl PassToken {
    pub fn new(value: String) -> PassToken {
        PassToken(value)
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

impl std::fmt::Display for PassToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One phase of a run. Construct via [`Phase::new`] or
/// `Phase { name: …, ..Default::default() }` — never by listing every field, so
/// adding a field stays a one-line diff instead of touching ~25 literal sites.
///
/// The cost of that convenience, for whoever adds the next field:
/// * The compiler no longer flags construction sites that ought to populate it.
///   Grep the `..Default::default()` sites and decide each one deliberately.
/// * No field here is `#[serde(default)]`, so deserialization REQUIRES all of
///   them. A new field without `#[serde(default)]` makes every existing
///   `state.json` fail to load → `load_run` exits 1 → the run STOPs. Mirror
///   `RunState`'s `#[serde(default, skip_serializing_if = "Option::is_none")]`
///   and add a back-compat test for it.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Phase {
    pub name: String,
    pub status: PhaseStatus,
    pub handoff_doc: Option<String>,
    pub herdr_session: Option<String>,
    pub pane_id: Option<String>,
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
}

impl Phase {
    /// A `Pending` phase named `name`, with every other field at its default.
    pub fn new(name: &str) -> Phase {
        Phase {
            name: name.to_owned(),
            ..Default::default()
        }
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
        serde_json::from_str(&fs::read_to_string(p)?).map_err(io::Error::other)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::ENV_LOCK;
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
                Phase {
                    name: "brainstorm".into(),
                    status: PhaseStatus::Done,
                    ..Default::default()
                },
                Phase::new("plan"),
            ],
            // A populated review_phases list must NOT influence pipeline progress:
            // it round-trips but `first_incomplete` (and `format_progress`) ignore it.
            review_phases: vec![Phase {
                name: "review:task-1:1:correctness".into(),
                status: PhaseStatus::Running,
                pane_id: Some("p1".into()),
                ..Default::default()
            }],
            gate: "spec".into(),
            cursor: 1,
            workspace: None,
            root_pane: None,
            project_dir: "/tmp/proj".into(),
            worktree_path: None,
            worktree_branch: None,
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
                .map(|i| Phase {
                    name: format!("phase-{i}-{}", "x".repeat(64)),
                    handoff_doc: Some("h".repeat(128)),
                    ..Default::default()
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
    fn pass_token_only_matches_a_non_empty_equal_marker() {
        let t = PassToken::new("abc-1".into());
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
        assert!(p.pane_id.is_none());
        assert_eq!(Phase::default().name, "");
        assert_eq!(PhaseStatus::default(), PhaseStatus::Pending);
    }

    #[test]
    fn find_phase_searches_both_lists() {
        let mk = |name: &str| Phase {
            name: name.into(),
            status: PhaseStatus::Running,
            ..Default::default()
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
