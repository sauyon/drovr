use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::{fs, io};

#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub enum PhaseStatus {
    Pending,
    Running,
    Done,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Phase {
    pub name: String,
    pub status: PhaseStatus,
    pub handoff_doc: Option<String>,
    pub herdr_session: Option<String>,
    pub pane_id: Option<String>,
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
    pub fn save(&self) -> io::Result<()> {
        let dir = run_dir(&self.name);
        fs::create_dir_all(&dir)?;
        fs::write(
            dir.join("state.json"),
            serde_json::to_string_pretty(self).map_err(io::Error::other)?,
        )?;
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
                    handoff_doc: None,
                    herdr_session: None,
                    pane_id: None,
                },
                Phase {
                    name: "plan".into(),
                    status: PhaseStatus::Pending,
                    handoff_doc: None,
                    herdr_session: None,
                    pane_id: None,
                },
            ],
            // A populated review_phases list must NOT influence pipeline progress:
            // it round-trips but `first_incomplete` (and `format_progress`) ignore it.
            review_phases: vec![Phase {
                name: "review:task-1:1:correctness".into(),
                status: PhaseStatus::Running,
                handoff_doc: None,
                herdr_session: None,
                pane_id: Some("p1".into()),
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
    fn find_phase_searches_both_lists() {
        let mk = |name: &str| Phase {
            name: name.into(),
            status: PhaseStatus::Running,
            handoff_doc: None,
            herdr_session: None,
            pane_id: None,
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
