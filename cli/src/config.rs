//! Backend-agnostic agent map + review angles.
//!
//! Loaded from `${XDG_CONFIG_HOME:-~/.config}/drovr/config.toml`, falling back to
//! baked-in defaults when the file is absent. Resolves a reviewer's launch command
//! and its read-only flag.
//!
//! User-defined agents override built-ins by key. Missing built-in agents and
//! missing optional fields on a built-in agent are filled in for compatibility.

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct AgentSpec {
    pub command: String,
    /// Read-only flag; absent → this agent cannot serve as a reviewer.
    #[serde(default)]
    pub readonly_flag: Option<String>,
    /// Flag used to pin the agent to the run's project directory.
    #[serde(default)]
    pub workspace_flag: Option<String>,
    /// Flag used to append the workspace-root guard prompt.
    #[serde(default)]
    pub system_prompt_flag: Option<String>,
    /// Arguments for non-interactive compression; `{project_dir}` is replaced.
    #[serde(default)]
    pub print_args: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Config {
    #[serde(default = "default_agent")]
    pub default_agent: String,
    #[serde(default = "default_angles")]
    pub angles: Vec<String>,
    #[serde(default = "default_agents")]
    pub agents: BTreeMap<String, AgentSpec>,
}

// Standalone default fns are REQUIRED (not `#[serde(default)]` bare): serde's bare default
// calls `BTreeMap::default()` (EMPTY) when the TOML omits `[agents]`, which would make the
// built-in claude entry vanish on any real config file and break `reviewer_launch("claude")`.
// Each default fn seeds the built-in value so an absent field falls back correctly.
fn default_agent() -> String {
    "claude".into()
}

fn default_angles() -> Vec<String> {
    vec![
        "correctness".into(),
        "security".into(),
        "error-handling".into(),
        "type-design".into(),
    ]
}

fn default_agents() -> BTreeMap<String, AgentSpec> {
    let mut m = BTreeMap::new();
    m.insert(
        "claude".to_string(),
        AgentSpec {
            command: "claude".into(),
            readonly_flag: Some("--permission-mode plan".into()),
            workspace_flag: Some("--add-dir".into()),
            system_prompt_flag: Some("--append-system-prompt".into()),
            print_args: Some(vec![
                "-p".into(),
                "--permission-mode".into(),
                "plan".into(),
                "--add-dir".into(),
                "{project_dir}".into(),
            ]),
        },
    );
    m.insert(
        "cursor".to_string(),
        AgentSpec {
            command: "agent".into(),
            readonly_flag: Some("--mode plan".into()),
            workspace_flag: Some("--workspace".into()),
            system_prompt_flag: None,
            print_args: Some(vec![
                "--print".into(),
                "--mode".into(),
                "plan".into(),
                "--workspace".into(),
                "{project_dir}".into(),
            ]),
        },
    );
    m.insert(
        "codex".to_string(),
        AgentSpec {
            command: "codex".into(),
            readonly_flag: Some("--sandbox read-only".into()),
            workspace_flag: Some("-C".into()),
            system_prompt_flag: None,
            print_args: Some(vec![
                "exec".into(),
                "--sandbox".into(),
                "read-only".into(),
                "-C".into(),
                "{project_dir}".into(),
                "-".into(),
            ]),
        },
    );
    m
}

fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Detect the agent backend invoking drovr.
///
/// `DROVR_AGENT` is an explicit escape hatch for agents without a stable
/// environment marker. Otherwise use markers exported by the major CLIs and
/// fall back to the configured default when drovr is run from an ordinary shell.
pub fn invoking_agent(config: &Config) -> String {
    if let Ok(agent) = std::env::var("DROVR_AGENT")
        && !agent.trim().is_empty()
    {
        return agent;
    }
    if std::env::var_os("CURSOR_AGENT").is_some() {
        return "cursor".into();
    }
    if std::env::var_os("CLAUDECODE").is_some() {
        return "claude".into();
    }
    if std::env::var_os("CODEX_THREAD_ID").is_some() {
        return "codex".into();
    }
    config.default_agent.clone()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            default_agent: default_agent(),
            angles: default_angles(),
            agents: default_agents(),
        }
    }
}

/// `${XDG_CONFIG_HOME:-$HOME/.config}/drovr/config.toml`
pub fn config_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // `HOME` unset (CI/containers) must not panic: fall back to a relative
            // `.config`, which `load_config` will simply see as NotFound → defaults.
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home).join(".config")
        });
    base.join("drovr").join("config.toml")
}

/// Load the config. Absent file → `Ok(Config::default())`; present-but-malformed → `Err`.
pub fn load_config() -> io::Result<Config> {
    let path = config_path();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(e),
    };
    let mut config: Config = toml::from_str(&text).map_err(io::Error::other)?;
    for (name, builtin) in default_agents() {
        if let Some(spec) = config.agents.get_mut(&name) {
            spec.readonly_flag = spec.readonly_flag.take().or(builtin.readonly_flag);
            spec.workspace_flag = spec.workspace_flag.take().or(builtin.workspace_flag);
            spec.system_prompt_flag = spec
                .system_prompt_flag
                .take()
                .or(builtin.system_prompt_flag);
            spec.print_args = spec.print_args.take().or(builtin.print_args);
        } else {
            config.agents.insert(name, builtin);
        }
    }
    Ok(config)
}

impl Config {
    fn agent(&self, name: &str) -> io::Result<&AgentSpec> {
        self.agents.get(name).ok_or_else(|| {
            io::Error::other(format!("unknown agent '{name}': not in config agent map"))
        })
    }

    /// Compose an agent launch command pinned to `project_dir`.
    pub fn launch(&self, agent: &str, project_dir: &str, readonly: bool) -> io::Result<String> {
        let spec = self.agent(agent)?;
        let mut command = spec.command.clone();
        if readonly {
            let flag = spec.readonly_flag.as_ref().ok_or_else(|| {
                io::Error::other(format!(
                    "agent '{agent}' has no readonly_flag; cannot serve as reviewer"
                ))
            })?;
            command.push(' ');
            command.push_str(flag);
        }
        if let Some(flag) = &spec.workspace_flag {
            command.push(' ');
            command.push_str(flag);
            command.push(' ');
            command.push_str(&shell_single_quote(project_dir));
        }
        if let Some(flag) = &spec.system_prompt_flag {
            let prompt = format!(
                "Your project root is {project_dir}. Treat it as the absolute workspace \
                 root: resolve every file path against it, and never read or edit files \
                 outside it. If this checkout is a git worktree that shares its .git with \
                 another checkout, ignore that outer checkout entirely — edits belong in \
                 {project_dir}."
            );
            command.push(' ');
            command.push_str(flag);
            command.push(' ');
            command.push_str(&shell_single_quote(&prompt));
        }
        Ok(command)
    }

    /// Resolve a non-interactive, read-only command for handoff compression.
    pub fn compressor(&self, agent: &str, project_dir: &str) -> io::Result<(String, Vec<String>)> {
        let spec = self.agent(agent)?;
        let args = spec.print_args.as_ref().ok_or_else(|| {
            io::Error::other(format!(
                "agent '{agent}' has no print_args; cannot compress a handoff"
            ))
        })?;
        Ok((
            spec.command.clone(),
            args.iter()
                .map(|arg| arg.replace("{project_dir}", project_dir))
                .collect(),
        ))
    }

    /// Return the composed reviewer launch command `"<command> <readonly_flag>"` for `agent`
    /// (defaults to `self.default_agent` when `agent` is `None`). Errors if the agent is
    /// unknown or has no `readonly_flag` ("cannot serve as reviewer").
    #[cfg(test)]
    pub fn reviewer_launch(&self, agent: Option<&str>) -> io::Result<String> {
        let name = agent.unwrap_or(&self.default_agent);
        let spec = self.agent(name)?;
        let flag = spec.readonly_flag.as_ref().ok_or_else(|| {
            io::Error::other(format!(
                "agent '{name}' has no readonly_flag; cannot serve as reviewer"
            ))
        })?;
        Ok(format!("{} {}", spec.command, flag))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::ENV_LOCK;

    // Sets XDG_CONFIG_HOME to `dir`. Caller must hold ENV_LOCK.
    fn set_config_home(dir: &std::path::Path) {
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir);
        }
    }

    #[test]
    fn absent_file_yields_defaults() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.default_agent, "claude");
        assert_eq!(
            cfg.angles,
            vec!["correctness", "security", "error-handling", "type-design"]
        );
        assert_eq!(
            cfg.reviewer_launch(None).unwrap(),
            "claude --permission-mode plan"
        );
        assert!(cfg.agents.contains_key("cursor"));
        assert_eq!(
            cfg.launch("cursor", "/tmp/my worktree", false).unwrap(),
            "agent --workspace '/tmp/my worktree'"
        );
        assert_eq!(
            cfg.launch("cursor", "/tmp/my worktree", true).unwrap(),
            "agent --mode plan --workspace '/tmp/my worktree'"
        );
    }

    #[test]
    fn detects_cursor_and_honors_explicit_override() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("DROVR_AGENT");
            std::env::set_var("CURSOR_AGENT", "1");
        }
        assert_eq!(invoking_agent(&Config::default()), "cursor");
        unsafe {
            std::env::set_var("DROVR_AGENT", "custom");
        }
        assert_eq!(invoking_agent(&Config::default()), "custom");
        unsafe {
            std::env::remove_var("DROVR_AGENT");
            std::env::remove_var("CURSOR_AGENT");
            std::env::remove_var("CLAUDECODE");
            std::env::set_var("CODEX_THREAD_ID", "thread");
        }
        assert_eq!(invoking_agent(&Config::default()), "codex");
        assert!(Config::default().agents.contains_key("codex"));
        unsafe {
            std::env::remove_var("CODEX_THREAD_ID");
        }
    }

    #[test]
    fn parses_spec_example_toml() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            r#"
default_agent = "claude"
angles = ["correctness", "security", "error-handling", "type-design"]

[agents.claude]
command = "claude"
readonly_flag = "--permission-mode plan"

[agents.codex]
command = "codex"
readonly_flag = "--sandbox read-only"
"#,
        )
        .unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        assert!(cfg.agents.contains_key("claude"));
        assert!(cfg.agents.contains_key("codex"));
        assert_eq!(
            cfg.reviewer_launch(Some("claude")).unwrap(),
            "claude --permission-mode plan"
        );
        assert_eq!(
            cfg.reviewer_launch(Some("codex")).unwrap(),
            "codex --sandbox read-only"
        );
    }

    #[test]
    fn file_omitting_agents_keeps_builtin_claude() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), "default_agent = \"claude\"\n").unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        assert_eq!(
            cfg.reviewer_launch(Some("claude")).unwrap(),
            "claude --permission-mode plan"
        );
    }

    #[test]
    fn user_agent_map_keeps_missing_builtins_and_fields() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "[agents.codex]\ncommand = \"codex\"\nreadonly_flag = \"--sandbox read-only\"\n",
        )
        .unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        assert!(cfg.reviewer_launch(Some("codex")).is_ok());
        assert!(cfg.reviewer_launch(Some("claude")).is_ok());
        assert!(cfg.reviewer_launch(Some("cursor")).is_ok());
        assert_eq!(
            cfg.launch("codex", "/tmp/project", false).unwrap(),
            "codex -C '/tmp/project'"
        );
    }

    #[test]
    fn reviewer_launch_errors_for_unknown_and_flagless_agents() {
        let cfg = Config {
            default_agent: "claude".into(),
            angles: default_angles(),
            agents: {
                let mut m = BTreeMap::new();
                m.insert(
                    "noflag".to_string(),
                    AgentSpec {
                        command: "noflag".into(),
                        readonly_flag: None,
                        workspace_flag: None,
                        system_prompt_flag: None,
                        print_args: None,
                    },
                );
                m
            },
        };
        assert!(cfg.reviewer_launch(Some("does-not-exist")).is_err());
        assert!(cfg.reviewer_launch(Some("noflag")).is_err());
    }

    #[test]
    fn reviewer_launch_none_resolves_overridden_default_agent() {
        let cfg = Config {
            default_agent: "codex".into(),
            angles: default_angles(),
            agents: {
                let mut m = BTreeMap::new();
                m.insert(
                    "codex".to_string(),
                    AgentSpec {
                        command: "codex".into(),
                        readonly_flag: Some("--sandbox read-only".into()),
                        workspace_flag: None,
                        system_prompt_flag: None,
                        print_args: None,
                    },
                );
                m
            },
        };
        // None must resolve self.default_agent ("codex"), not the built-in "claude".
        assert_eq!(
            cfg.reviewer_launch(None).unwrap(),
            "codex --sandbox read-only"
        );
    }

    #[test]
    fn reviewer_launch_none_errors_when_default_agent_lacks_flag() {
        let cfg = Config {
            default_agent: "noflag".into(),
            angles: default_angles(),
            agents: {
                let mut m = BTreeMap::new();
                m.insert(
                    "noflag".to_string(),
                    AgentSpec {
                        command: "noflag".into(),
                        readonly_flag: None,
                        workspace_flag: None,
                        system_prompt_flag: None,
                        print_args: None,
                    },
                );
                m
            },
        };
        assert!(cfg.reviewer_launch(None).is_err());
    }

    #[test]
    fn malformed_toml_errors() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), "this is = = not valid toml [[[").unwrap();
        set_config_home(tmp.path());

        assert!(load_config().is_err());
    }
}
