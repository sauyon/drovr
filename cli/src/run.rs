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
    pub gate: String, pub cursor: usize,
    /// The herdr workspace id created for this run (set by `drovr new`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
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
            ], gate:"spec".into(), cursor:1, workspace: None, project_dir: "/tmp/proj".into() };
        s.save().unwrap();
        let loaded = RunState::load("demo").unwrap();
        assert_eq!(loaded.phases.len(), 2);
        assert_eq!(loaded.first_incomplete(), Some(1));
    }
}
