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
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::shell::shell_single_quote;

/// How a backend is handed an MCP server.
///
/// The two supported mechanisms are genuinely different, so they are separate
/// variants rather than a pile of optional flags that could be combined into
/// nonsense: claude takes the server config *file* on its command line, while
/// cursor has no such flag at all and only reads a fixed path inside the project
/// directory. A backend either names the file or it does not — never both.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "mechanism", rename_all = "kebab-case")]
pub enum McpDelivery {
    /// The config file is named on the command line: `<flag> <path>`, plus any
    /// `extra_flags` (claude: `--mcp-config <path> --strict-mcp-config`, the
    /// second flag confining the agent to exactly the servers drovr passed).
    ConfigFlag {
        flag: String,
        #[serde(default)]
        extra_flags: Vec<String>,
    },
    /// The backend reads servers only from a fixed project-relative path
    /// (cursor: `.cursor/mcp.json`); `extra_flags` carries whatever makes it
    /// trust that file without a prompt (`--approve-mcps`).
    ProjectFile {
        path: String,
        #[serde(default)]
        extra_flags: Vec<String>,
    },
}

impl McpDelivery {
    /// Where drovr must write the server config for this mechanism. A flag
    /// backend reads it from drovr's own run directory, out of the project
    /// entirely; a project-file backend only ever looks inside the project.
    ///
    /// `stem` names the file for a flag backend (the reviewers of one task share
    /// one server, so it is the task name).
    pub fn config_path(&self, run_dir: &Path, project_dir: &Path, stem: &str) -> PathBuf {
        match self {
            McpDelivery::ConfigFlag { .. } => run_dir.join(format!("{stem}-review-mcp.json")),
            McpDelivery::ProjectFile { path, .. } => project_dir.join(path),
        }
    }

    /// The project-relative path this mechanism writes into the user's checkout,
    /// if any. `Some` means the file is visible to git and needs excluding.
    pub fn project_relative_path(&self) -> Option<&str> {
        match self {
            McpDelivery::ConfigFlag { .. } => None,
            McpDelivery::ProjectFile { path, .. } => Some(path),
        }
    }

    fn extra_flags(&self) -> &[String] {
        match self {
            McpDelivery::ConfigFlag { extra_flags, .. }
            | McpDelivery::ProjectFile { extra_flags, .. } => extra_flags,
        }
    }
}

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
    /// Flag used to select a model for read-only reviews.
    #[serde(default)]
    pub model_flag: Option<String>,
    /// Model selected for read-only reviews. Absent means backend default.
    #[serde(default)]
    pub review_model: Option<String>,
    /// How this backend is given an MCP server. Absent → it cannot be handed
    /// one, so it cannot serve on the review panel (whose findings channel is a
    /// tool call).
    #[serde(default)]
    pub mcp: Option<McpDelivery>,
}

/// Controls the SessionStart reflex the `session-start` hook injects (see
/// `drovr reflex`). All fields are optional; an absent `[reflex]` table yields
/// the built-in reflex unchanged.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct ReflexConfig {
    /// Master switch. `false` suppresses the reflex entirely for human sessions
    /// (the `DROVR_PHASE` phase-suppression is separate and always applies).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Overrides the framing text placed before the skill body inside the
    /// `<EXTREMELY_IMPORTANT>` wrapper. Absent → the built-in framing.
    #[serde(default)]
    pub preamble: Option<String>,
    /// Per-section overrides keyed by the section name tagged in the skill
    /// markdown (`<!-- reflex:section:NAME -->`). A section absent from this map
    /// defaults to enabled; `NAME = false` omits that section from the reflex.
    #[serde(default)]
    pub sections: BTreeMap<String, bool>,
}

impl Default for ReflexConfig {
    fn default() -> Self {
        ReflexConfig {
            enabled: true,
            preamble: None,
            sections: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Config {
    #[serde(default = "default_agent")]
    pub default_agent: String,
    /// Backend used for automated review panels. When omitted, prefer Cursor's
    /// `agent` backend if its command is available, then fall back to the run backend.
    #[serde(default)]
    pub review_agent: Option<String>,
    #[serde(default = "default_angles")]
    pub angles: Vec<String>,
    /// Default host/address `drovr serve` binds to when `--host` is omitted.
    #[serde(default = "default_serve_host")]
    pub serve_host: String,
    /// When true, `drovr new` isolates each run in a git worktree
    /// (`.drovr/wt/<run>` on branch `drovr/<run>`) unless `--no-worktree` is
    /// passed. `--worktree` overrides this to on per-run. **On by default** —
    /// isolation keeps a run's edits off the checkout you launched it from, so
    /// `default_true` (not a bare `#[serde(default)]`, which would yield `false`
    /// whenever a config file omits the key).
    #[serde(default = "default_true")]
    pub worktree: bool,
    /// SessionStart reflex configuration (see [`ReflexConfig`]).
    #[serde(default)]
    pub reflex: ReflexConfig,
    #[serde(default = "default_agents")]
    pub agents: BTreeMap<String, AgentSpec>,
}

// Standalone default fns are REQUIRED (not `#[serde(default)]` bare): serde's bare default
// calls `BTreeMap::default()` (EMPTY) when the TOML omits `[agents]`, which would make the
// built-in claude entry vanish on any real config file and break `reviewer_launch("claude")`.
// The same trap applies to non-empty String defaults like `serve_host`: bare `#[serde(default)]`
// yields `String::default()` (`""`), not `"127.0.0.1"`, so it needs a named fn too.
// Each default fn seeds the built-in value so an absent field falls back correctly.
fn default_agent() -> String {
    "claude".into()
}

fn default_serve_host() -> String {
    "127.0.0.1".into()
}

// Named `true` default for `ReflexConfig::enabled`: a bare `#[serde(default)]`
// on a bool yields `false`, which would silently disable the reflex whenever a
// `[reflex]` table is present but omits `enabled`.
fn default_true() -> bool {
    true
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
            model_flag: Some("--model".into()),
            review_model: None,
            mcp: Some(McpDelivery::ConfigFlag {
                flag: "--mcp-config".into(),
                // `--strict-mcp-config`: exactly the servers drovr passed, none of
                // the user's. `--allowedTools`: plan mode gates tool use, so the one
                // tool the panel depends on is pre-allowed — and only that one, whose
                // single effect is drovr writing the findings file.
                // `--allowedTools` is VARIADIC: given as two argv words it swallows
                // whatever follows, so it is passed in the `=` form, which closes the
                // option and cannot consume the next flag.
                extra_flags: vec![
                    "--strict-mcp-config".into(),
                    format!(
                        "--allowedTools={}",
                        crate::mcp_findings::qualified_tool_name()
                    ),
                ],
            }),
        },
    );
    m.insert(
        "cursor".to_string(),
        AgentSpec {
            command: "agent".into(),
            readonly_flag: Some("--mode plan".into()),
            workspace_flag: Some("--workspace".into()),
            system_prompt_flag: None,
            model_flag: Some("--model".into()),
            review_model: Some("composer-2.5".into()),
            // No per-launch scoping exists: servers come from the project's
            // `.cursor/mcp.json`, so drovr writes that file instead.
            //
            // `--approve-mcps` approves EVERY server in that file — there is no
            // per-server form. That is only safe because
            // `code_review::write_mcp_config` makes drovr's findings server the sole
            // entry, backing up anything it displaces. Preserving other servers here
            // would auto-approve them for a read-only reviewer, and `.cursor/mcp.json`
            // is a path the repository under review can simply commit. If that write
            // is ever made to merge again, this flag has to go with it.
            mcp: Some(McpDelivery::ProjectFile {
                path: ".cursor/mcp.json".into(),
                extra_flags: vec!["--approve-mcps".into()],
            }),
        },
    );
    m.insert(
        "codex".to_string(),
        AgentSpec {
            command: "codex".into(),
            readonly_flag: Some("--sandbox read-only".into()),
            workspace_flag: Some("-C".into()),
            system_prompt_flag: None,
            model_flag: Some("-m".into()),
            review_model: None,
            mcp: None,
        },
    );
    m
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
            review_agent: None,
            angles: default_angles(),
            serve_host: default_serve_host(),
            worktree: default_true(),
            reflex: ReflexConfig::default(),
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
    // An empty `serve_host` is a config mistake, not a request for the default
    // (serde's default only fires for an *absent* key). Reject it here so the
    // failure is a clear config error rather than an opaque bind error later.
    if config.serve_host.trim().is_empty() {
        return Err(io::Error::other("serve_host must not be empty"));
    }
    // Reject relative-path commands before any of them can be spawned. A
    // hostile repo under review (or tampered config) must not be able to point
    // drovr at a CWD-relative binary.
    for (name, spec) in &config.agents {
        validate_command(&spec.command)
            .map_err(|e| io::Error::other(format!("agent '{name}': {e}")))?;
        if let Some(delivery) = &spec.mcp {
            validate_delivery(delivery)
                .map_err(|e| io::Error::other(format!("agent '{name}': {e}")))?;
        }
    }
    for (name, builtin) in default_agents() {
        if let Some(spec) = config.agents.get_mut(&name) {
            spec.readonly_flag = spec.readonly_flag.take().or(builtin.readonly_flag);
            spec.workspace_flag = spec.workspace_flag.take().or(builtin.workspace_flag);
            spec.system_prompt_flag = spec
                .system_prompt_flag
                .take()
                .or(builtin.system_prompt_flag);
            spec.model_flag = spec.model_flag.take().or(builtin.model_flag);
            spec.review_model = spec.review_model.take().or(builtin.review_model);
            spec.mcp = spec.mcp.take().or(builtin.mcp);
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

    /// Select the backend for an automated review panel.
    ///
    /// An explicit `review_agent` is always honored. Otherwise Cursor's `agent`
    /// backend is preferred when its configured command is executable and its
    /// herdr integration is available; otherwise use the backend captured by the
    /// run (or `default_agent`).
    pub fn review_agent_for(
        &self,
        run_agent: Option<&str>,
        cursor_integration_available: bool,
    ) -> io::Result<String> {
        if let Some(name) = self.review_agent.as_deref() {
            self.agent(name)?;
            return Ok(name.to_owned());
        }

        if let Some(cursor) = self.agents.get("cursor")
            && cursor.readonly_flag.is_some()
            && command_available(&cursor.command)
            && cursor_integration_available
        {
            return Ok("cursor".into());
        }

        let name = run_agent.unwrap_or(&self.default_agent);
        self.agent(name)?;
        Ok(name.to_owned())
    }

    /// How `agent` is handed an MCP server, if it can be. The caller needs this
    /// *before* [`Config::launch`]: the config file has to exist at
    /// [`McpDelivery::config_path`] by the time the agent starts.
    pub fn mcp_delivery(&self, agent: &str) -> io::Result<Option<&McpDelivery>> {
        Ok(self.agent(agent)?.mcp.as_ref())
    }

    /// Compose an agent launch command pinned to `project_dir`.
    ///
    /// `mcp_config` is the path drovr wrote the server config to (see
    /// [`Config::mcp_delivery`]). How — or whether — that path reaches the agent
    /// depends on its [`McpDelivery`]: a flag backend gets it on the command
    /// line, a project-file backend only gets the flags that make it read the
    /// file it already knows about.
    pub fn launch(
        &self,
        agent: &str,
        project_dir: &str,
        readonly: bool,
        mcp_config: Option<&Path>,
    ) -> io::Result<String> {
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
            if let Some(model) = &spec.review_model {
                let model_flag = spec.model_flag.as_ref().ok_or_else(|| {
                    io::Error::other(format!(
                        "agent '{agent}' has a review_model but no model_flag"
                    ))
                })?;
                command.push(' ');
                command.push_str(model_flag);
                command.push(' ');
                command.push_str(&shell_single_quote(model));
            }
        }
        if let Some(flag) = &spec.workspace_flag {
            command.push(' ');
            command.push_str(flag);
            command.push(' ');
            command.push_str(&shell_single_quote(project_dir));
        }
        if let Some(path) = mcp_config {
            let delivery = spec.mcp.as_ref().ok_or_else(|| {
                io::Error::other(format!(
                    "agent '{agent}' has no `mcp` mechanism; it cannot be given an \
                     MCP server (so it cannot serve on the review panel)"
                ))
            })?;
            if let McpDelivery::ConfigFlag { flag, .. } = delivery {
                command.push(' ');
                command.push_str(flag);
                command.push(' ');
                command.push_str(&shell_single_quote(&path.display().to_string()));
            }
            for flag in delivery.extra_flags() {
                command.push(' ');
                command.push_str(flag);
            }
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

/// Reject agent commands that would resolve against an untrusted working
/// directory. drovr often runs with its CWD inside the repository under review;
/// a relative-path command (`./git`, `../tool`, `bin/agent`) resolves against
/// that CWD, so a hostile repo (or a tampered config) could drop a lookalike
/// binary and have drovr execute it. A bare name (`claude`) is resolved via
/// `$PATH` — a trusted, user-controlled search path — and an absolute path names
/// exactly one file; both are allowed. Anything relative-but-pathful is not.
fn validate_command(command: &str) -> io::Result<()> {
    if command.trim().is_empty() {
        return Err(io::Error::other("agent command must not be empty"));
    }
    // Validate exactly what gets spawned. Surrounding whitespace is never a
    // valid command and would otherwise pass the path check below (a single
    // component) only to fail later with a cryptic spawn error — reject it here.
    if command != command.trim() {
        return Err(io::Error::other(format!(
            "agent command '{command}' has leading or trailing whitespace"
        )));
    }
    let path = std::path::Path::new(command);
    if path.components().count() > 1 && !path.is_absolute() {
        return Err(io::Error::other(format!(
            "agent command '{command}' is a relative path; use a bare name \
             (resolved via $PATH) or an absolute path"
        )));
    }
    Ok(())
}

/// Reject an MCP mechanism whose file would land outside the directory it is
/// meant to. A `ProjectFile` path is joined onto the project dir, so an absolute
/// path or a `..` component would have drovr write the server config anywhere on
/// disk — under a name a hostile config chooses.
fn validate_delivery(delivery: &McpDelivery) -> io::Result<()> {
    let Some(path) = delivery.project_relative_path() else {
        return Ok(());
    };
    if path.trim().is_empty() {
        return Err(io::Error::other("mcp `path` must not be empty"));
    }
    let p = Path::new(path);
    if p.is_absolute()
        || p.components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(io::Error::other(format!(
            "mcp `path` '{path}' must be a relative path inside the project"
        )));
    }
    Ok(())
}

fn command_available(command: &str) -> bool {
    let path = std::path::Path::new(command);
    if path.components().count() > 1 {
        return executable_file(path);
    }

    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| executable_file(&dir.join(command))))
        .unwrap_or(false)
}

fn executable_file(path: &std::path::Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
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
        assert_eq!(cfg.review_agent, None);
        assert_eq!(
            cfg.angles,
            vec!["correctness", "security", "error-handling", "type-design"]
        );
        assert_eq!(cfg.serve_host, "127.0.0.1");
        // Reflex defaults: on, no preamble override, no section overrides.
        assert!(cfg.reflex.enabled);
        assert_eq!(cfg.reflex.preamble, None);
        assert!(cfg.reflex.sections.is_empty());
        assert_eq!(
            cfg.reviewer_launch(None).unwrap(),
            "claude --permission-mode plan"
        );
        assert!(cfg.agents.contains_key("cursor"));
        assert_eq!(
            cfg.launch("cursor", "/tmp/my worktree", false, None)
                .unwrap(),
            "agent --workspace '/tmp/my worktree'"
        );
        assert_eq!(
            cfg.launch("cursor", "/tmp/my worktree", true, None)
                .unwrap(),
            "agent --mode plan --model 'composer-2.5' --workspace '/tmp/my worktree'"
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

    /// claude carries the server config on its command line, so the path drovr
    /// wrote must appear there — with `--strict-mcp-config`, so the reviewer gets
    /// drovr's one tool and nothing the user happens to have configured globally.
    #[test]
    fn a_config_flag_backend_takes_the_path_on_its_command_line() {
        let cfg = Config::default();
        let path = std::path::Path::new("/data/runs/r/task-1-review-mcp.json");
        let cmd = cfg.launch("claude", "/tmp/proj", true, Some(path)).unwrap();
        assert!(
            cmd.contains("--mcp-config '/data/runs/r/task-1-review-mcp.json'"),
            "{cmd}"
        );
        assert!(cmd.contains("--strict-mcp-config"), "{cmd}");
    }

    /// cursor has no such flag: it only reads `.cursor/mcp.json` inside the
    /// project. Putting the path on its command line would be a flag it rejects,
    /// so the launch carries only what makes it trust that file.
    #[test]
    fn a_project_file_backend_gets_flags_but_never_the_path() {
        let cfg = Config::default();
        let path = std::path::Path::new("/tmp/proj/.cursor/mcp.json");
        let cmd = cfg.launch("cursor", "/tmp/proj", true, Some(path)).unwrap();
        assert!(cmd.contains("--approve-mcps"), "{cmd}");
        assert!(
            !cmd.contains("mcp.json"),
            "cursor has no flag that can carry the path: {cmd}"
        );
    }

    /// claude's plan mode gates tool use, so the one tool the panel depends on is
    /// pre-allowed by name — and only that one. The name is derived from the server
    /// key drovr registers, so the allowlist cannot drift from what is served.
    #[test]
    fn the_claude_reviewer_launch_pre_allows_exactly_the_findings_tool() {
        let cfg = Config::default();
        let cmd = cfg
            .launch(
                "claude",
                "/tmp/proj",
                true,
                Some(std::path::Path::new("/x.json")),
            )
            .unwrap();
        let tool = crate::mcp_findings::qualified_tool_name();
        assert!(cmd.contains(&format!("--allowedTools={tool}")), "{cmd}");
        assert!(
            !cmd.contains(&format!("--allowedTools {tool}")),
            "the flag is variadic, so it must use the `=` form or it swallows the \
             argument after it: {cmd}"
        );
        assert_eq!(
            cmd.matches("--allowedTools").count(),
            1,
            "exactly one carve-out, not a list that could grow: {cmd}"
        );
    }

    #[test]
    fn no_mcp_config_leaves_the_launch_untouched() {
        let cfg = Config::default();
        let cmd = cfg.launch("claude", "/tmp/proj", true, None).unwrap();
        assert!(!cmd.contains("--mcp-config"), "{cmd}");
        assert!(!cmd.contains("--strict-mcp-config"), "{cmd}");
    }

    /// A backend with no MCP mechanism cannot be handed one. Failing here beats
    /// launching a reviewer that has no way to deliver findings.
    #[test]
    fn a_backend_without_a_mechanism_refuses_an_mcp_config() {
        let cfg = Config::default();
        assert!(cfg.mcp_delivery("codex").unwrap().is_none());
        let err = cfg
            .launch(
                "codex",
                "/tmp/proj",
                true,
                Some(std::path::Path::new("/tmp/x.json")),
            )
            .expect_err("codex has no MCP mechanism");
        assert!(err.to_string().contains("codex"), "{err}");
    }

    /// The two mechanisms put the file in different places: drovr's own run dir
    /// for a flag backend, inside the project for one that only reads a fixed path.
    #[test]
    fn each_mechanism_places_its_config_where_that_backend_looks() {
        let cfg = Config::default();
        let run_dir = std::path::Path::new("/data/runs/r");
        let project = std::path::Path::new("/tmp/proj");
        assert_eq!(
            cfg.mcp_delivery("claude")
                .unwrap()
                .unwrap()
                .config_path(run_dir, project, "task-1"),
            run_dir.join("task-1-review-mcp.json")
        );
        let cursor = cfg.mcp_delivery("cursor").unwrap().unwrap();
        assert_eq!(
            cursor.config_path(run_dir, project, "task-1"),
            project.join(".cursor/mcp.json")
        );
        assert_eq!(cursor.project_relative_path(), Some(".cursor/mcp.json"));
        assert_eq!(
            cfg.mcp_delivery("claude")
                .unwrap()
                .unwrap()
                .project_relative_path(),
            None,
            "a flag backend's config never lands in the project"
        );
    }

    #[test]
    fn mcp_mechanisms_parse_from_config_and_are_merged_per_agent() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            r#"
[agents.mine]
command = "mine"
readonly_flag = "--ro"

[agents.mine.mcp]
mechanism = "config-flag"
flag = "--servers"
extra_flags = ["--only-these"]

[agents.cursor]
command = "agent"
"#,
        )
        .unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        let cmd = cfg
            .launch(
                "mine",
                "/tmp/p",
                true,
                Some(std::path::Path::new("/tmp/s.json")),
            )
            .unwrap();
        assert!(cmd.contains("--servers '/tmp/s.json'"), "{cmd}");
        assert!(cmd.contains("--only-these"), "{cmd}");
        // An agent that overrides a built-in without restating `mcp` keeps it.
        assert_eq!(
            cfg.mcp_delivery("cursor")
                .unwrap()
                .unwrap()
                .project_relative_path(),
            Some(".cursor/mcp.json")
        );
    }

    /// The project-relative path is joined onto the project dir, so an absolute
    /// or traversing value would write outside the project. Reject it at load.
    #[test]
    fn a_project_file_path_that_escapes_the_project_is_rejected() {
        let _lock = ENV_LOCK.lock().unwrap();
        for bad in ["/etc/mcp.json", "../../.cursor/mcp.json"] {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path().join("drovr");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("config.toml"),
                format!(
                    "[agents.mine]\ncommand = \"mine\"\n\n[agents.mine.mcp]\n\
                     mechanism = \"project-file\"\npath = {bad:?}\n"
                ),
            )
            .unwrap();
            set_config_home(tmp.path());
            let err = load_config().expect_err("path '{bad}' must be rejected");
            assert!(err.to_string().contains("mine"), "{err}");
        }
    }

    #[test]
    fn validate_command_accepts_bare_and_absolute() {
        assert!(validate_command("claude").is_ok());
        assert!(validate_command("cursor-agent").is_ok());
        assert!(validate_command("/usr/local/bin/claude").is_ok());
    }

    #[test]
    fn validate_command_rejects_relative_and_empty() {
        assert!(validate_command("./claude").is_err());
        assert!(validate_command("../bin/claude").is_err());
        assert!(validate_command("bin/agent").is_err());
        assert!(validate_command("").is_err());
        assert!(validate_command("   ").is_err());
        // Surrounding whitespace: rejected clearly rather than passing here and
        // failing later at spawn time.
        assert!(validate_command(" claude").is_err());
        assert!(validate_command("claude ").is_err());
    }

    #[test]
    fn load_config_rejects_relative_command() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            r#"
default_agent = "evil"

[agents.evil]
command = "./git"
"#,
        )
        .unwrap();
        set_config_home(tmp.path());

        let err = load_config().expect_err("relative-path command must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("evil"), "error should name the agent: {msg}");
        assert!(
            msg.contains("relative path"),
            "error should explain the reason: {msg}"
        );
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
review_agent = "codex"
serve_host = "0.0.0.0"
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
        assert_eq!(cfg.review_agent.as_deref(), Some("codex"));
        // A file-set serve_host propagates through load_config (not just the default path).
        assert_eq!(cfg.serve_host, "0.0.0.0");
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
    fn worktree_isolation_defaults_on() {
        // On by default in both paths: no config file (Config::default) and a
        // config file that omits the key (serde default).
        assert!(Config::default().worktree, "Config::default should isolate");

        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), "default_agent = \"claude\"\n").unwrap();
        set_config_home(tmp.path());
        assert!(
            load_config().unwrap().worktree,
            "a config omitting `worktree` should still isolate"
        );

        // Explicit opt-out is honored.
        std::fs::write(dir.join("config.toml"), "worktree = false\n").unwrap();
        assert!(
            !load_config().unwrap().worktree,
            "worktree = false must win"
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
            cfg.launch("codex", "/tmp/project", false, None).unwrap(),
            "codex -C '/tmp/project'"
        );
    }

    #[test]
    fn reviewer_launch_errors_for_unknown_and_flagless_agents() {
        let cfg = Config {
            default_agent: "claude".into(),
            review_agent: None,
            angles: default_angles(),
            serve_host: default_serve_host(),
            worktree: false,
            reflex: ReflexConfig::default(),
            agents: {
                let mut m = BTreeMap::new();
                m.insert(
                    "noflag".to_string(),
                    AgentSpec {
                        command: "noflag".into(),
                        readonly_flag: None,
                        workspace_flag: None,
                        system_prompt_flag: None,
                        model_flag: None,
                        review_model: None,
                        mcp: None,
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
            review_agent: None,
            angles: default_angles(),
            serve_host: default_serve_host(),
            worktree: false,
            reflex: ReflexConfig::default(),
            agents: {
                let mut m = BTreeMap::new();
                m.insert(
                    "codex".to_string(),
                    AgentSpec {
                        command: "codex".into(),
                        readonly_flag: Some("--sandbox read-only".into()),
                        workspace_flag: None,
                        system_prompt_flag: None,
                        model_flag: None,
                        review_model: None,
                        mcp: None,
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
            review_agent: None,
            angles: default_angles(),
            serve_host: default_serve_host(),
            worktree: false,
            reflex: ReflexConfig::default(),
            agents: {
                let mut m = BTreeMap::new();
                m.insert(
                    "noflag".to_string(),
                    AgentSpec {
                        command: "noflag".into(),
                        readonly_flag: None,
                        workspace_flag: None,
                        system_prompt_flag: None,
                        model_flag: None,
                        review_model: None,
                        mcp: None,
                    },
                );
                m
            },
        };
        assert!(cfg.reviewer_launch(None).is_err());
    }

    #[test]
    fn reflex_config_parses_full_table() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            r#"
[reflex]
enabled = false
preamble = "Custom framing text."

[reflex.sections]
always-review = false
escalation = true
"#,
        )
        .unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        assert!(!cfg.reflex.enabled);
        assert_eq!(cfg.reflex.preamble.as_deref(), Some("Custom framing text."));
        assert_eq!(cfg.reflex.sections.get("always-review"), Some(&false));
        assert_eq!(cfg.reflex.sections.get("escalation"), Some(&true));
        // An unlisted section has no override entry.
        assert_eq!(cfg.reflex.sections.get("single-writer"), None);
    }

    #[test]
    fn reflex_table_present_but_enabled_omitted_defaults_true() {
        // The bare-`#[serde(default)]`-on-bool trap: an absent `enabled` under a
        // present `[reflex]` table must fall back to `true`, not `bool::default()`.
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "[reflex]\npreamble = \"only a preamble here\"\n",
        )
        .unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        assert!(cfg.reflex.enabled);
        assert_eq!(cfg.reflex.preamble.as_deref(), Some("only a preamble here"));
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

    #[test]
    fn empty_serve_host_errors() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        // Explicit empty string is a mistake, not a request for the default.
        std::fs::write(dir.join("config.toml"), "serve_host = \"\"\n").unwrap();
        set_config_home(tmp.path());

        assert!(load_config().is_err());
    }

    #[test]
    fn review_agent_prefers_available_cursor_command() {
        let dir = tempfile::tempdir().unwrap();
        let agent = dir.path().join("agent");
        std::fs::write(&agent, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            let mut permissions = std::fs::metadata(&agent).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&agent, permissions).unwrap();
        }

        let mut cfg = Config::default();
        cfg.agents.get_mut("cursor").unwrap().command = agent.to_string_lossy().into_owned();
        assert_eq!(
            cfg.review_agent_for(Some("claude"), true).unwrap(),
            "cursor",
        );
    }

    #[test]
    fn review_agent_falls_back_and_honors_override() {
        let mut cfg = Config::default();
        cfg.agents.get_mut("cursor").unwrap().command = "/definitely/not/a/real/drovr-agent".into();
        assert_eq!(cfg.review_agent_for(Some("codex"), true).unwrap(), "codex",);

        cfg.review_agent = Some("claude".into());
        assert_eq!(cfg.review_agent_for(Some("codex"), true).unwrap(), "claude",);
    }

    #[test]
    fn review_agent_requires_cursor_integration_for_auto_preference() {
        let dir = tempfile::tempdir().unwrap();
        let agent = dir.path().join("agent");
        std::fs::write(&agent, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            let mut permissions = std::fs::metadata(&agent).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&agent, permissions).unwrap();
        }

        let mut cfg = Config::default();
        cfg.agents.get_mut("cursor").unwrap().command = agent.to_string_lossy().into_owned();
        assert_eq!(
            cfg.review_agent_for(Some("claude"), false).unwrap(),
            "claude",
        );
    }
}
