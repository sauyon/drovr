use serde::{Serialize, Deserialize};
use std::path::PathBuf;
use std::{fs, io};

#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub enum PhaseStatus { Pending, Running, Done, Failed }

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Phase {
    pub name: String, pub status: PhaseStatus,
    pub handoff_doc: Option<String>, pub herdr_session: Option<String>, pub pane_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RunState {
    pub name: String, pub task: String, pub phases: Vec<Phase>,
    /// Reviewer phases (`review:<task>:<iter>:<angle>`), kept OUT of `phases` so
    /// they never pollute pipeline progress: `first_incomplete` and
    /// `format_progress` (main.rs) walk `phases` only, and that omission IS the
    /// isolation. Only `find_phase` (and the marker/pane-id lookups that delegate
    /// to it) consult this list. `#[serde(default)]` so pre-existing state.json
    /// files (written before this field) load with an empty list.
    #[serde(default)]
    pub review_phases: Vec<Phase>,
    pub gate: String, pub cursor: usize,
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
}

pub fn run_dir(name: &str) -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap()).join(".local/share"));
    base.join("drovr").join("runs").join(name)
}

impl RunState {
    pub fn load(name: &str) -> io::Result<RunState> {
        let p = run_dir(name).join("state.json");
        serde_json::from_str(&fs::read_to_string(p)?)
            .map_err(io::Error::other)
    }
    pub fn save(&self) -> io::Result<()> {
        let dir = run_dir(&self.name);
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("state.json"),
            serde_json::to_string_pretty(self)
                .map_err(io::Error::other)?)?;
        Ok(())
    }
    pub fn first_incomplete(&self) -> Option<usize> {
        self.phases.iter().position(|p| p.status != PhaseStatus::Done)
    }
    /// Look up a phase by name across BOTH `phases` and `review_phases`. Reviewer
    /// lookups (marker-drop, seed injection) need to resolve names living in
    /// `review_phases`; pipeline progress deliberately does NOT use this (it stays
    /// bound to `phases` only — see `first_incomplete`). Searches `phases` first,
    /// then `review_phases`.
    pub fn find_phase(&self, name: &str) -> Option<&Phase> {
        self.phases.iter()
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
        unsafe { std::env::set_var("XDG_DATA_HOME", "/tmp/drovr-xdg-test"); }
        assert_eq!(run_dir("demo"), PathBuf::from("/tmp/drovr-xdg-test/drovr/runs/demo"));
    }
    #[test]
    fn state_roundtrips_and_finds_first_incomplete() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("XDG_DATA_HOME", "/tmp/drovr-xdg-test2"); }
        let s = RunState { name:"demo".into(), task:"t".into(),
            phases: vec![
                Phase{name:"brainstorm".into(), status:PhaseStatus::Done, handoff_doc:None, herdr_session:None, pane_id:None},
                Phase{name:"plan".into(), status:PhaseStatus::Pending, handoff_doc:None, herdr_session:None, pane_id:None},
            ],
            // A populated review_phases list must NOT influence pipeline progress:
            // it round-trips but `first_incomplete` (and `format_progress`) ignore it.
            review_phases: vec![
                Phase{name:"review:task-1:1:correctness".into(), status:PhaseStatus::Running, handoff_doc:None, herdr_session:None, pane_id:Some("p1".into())},
            ],
            gate:"spec".into(), cursor:1, workspace: None, root_pane: None, project_dir: "/tmp/proj".into() };
        s.save().unwrap();
        let loaded = RunState::load("demo").unwrap();
        assert_eq!(loaded.phases.len(), 2);
        assert_eq!(loaded.review_phases.len(), 1, "review_phases must round-trip");
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
        assert!(loaded.review_phases.is_empty(), "absent review_phases must default to []");
    }

    #[test]
    fn find_phase_searches_both_lists() {
        let mk = |name: &str| Phase {
            name: name.into(), status: PhaseStatus::Running,
            handoff_doc: None, herdr_session: None, pane_id: None,
        };
        let s = RunState {
            name: "r".into(), task: "t".into(),
            phases: vec![mk("plan")],
            review_phases: vec![mk("review:task-1:1:correctness")],
            gate: "spec".into(), cursor: 0, workspace: None, root_pane: None,
            project_dir: "/tmp/proj".into(),
        };
        assert_eq!(s.find_phase("plan").map(|p| p.name.as_str()), Some("plan"));
        assert_eq!(
            s.find_phase("review:task-1:1:correctness").map(|p| p.name.as_str()),
            Some("review:task-1:1:correctness"),
            "find_phase must also search review_phases"
        );
        assert!(s.find_phase("nonexistent").is_none());
    }
}
