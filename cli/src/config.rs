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
use std::path::PathBuf;

use crate::herdr::SessionId;
use crate::shell::shell_single_quote;

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
    /// Flag that resumes a prior session, composed as
    /// `<command> … <flag> '<id>'` — where the rest of the flags go. Absent →
    /// this agent offers no flag-shaped resume.
    ///
    /// Mutually exclusive with [`AgentSpec::resume_subcommand`]; setting both is
    /// a config error (see [`validate_resume`]).
    #[serde(default)]
    pub resume_flag: Option<String>,
    /// Subcommand that resumes a prior session, composed as
    /// `<command> <subcommand> '<id>' …` — immediately after the command,
    /// because a subcommand is not a flag and cannot follow one.
    ///
    /// No built-in agent sets this. `codex resume <id>` is the known shape, but
    /// codex was not installed on the machine this was written on, so its
    /// argument ordering could not be verified — and an unverified guess that
    /// composes a wrong command line is worse than falling back to a reseed. A
    /// codex user opts in explicitly:
    ///
    /// ```toml
    /// [agents.codex]
    /// resume_subcommand = "resume"
    /// ```
    #[serde(default)]
    pub resume_subcommand: Option<String>,
}

/// How an agent is asked to bring a prior session back. Exactly one shape per
/// agent — resume is not one shape across backends:
///
/// * claude `-r, --resume [value]` and cursor `agent --resume [chatId]` are
///   FLAGS whose value is optional;
/// * codex takes a `codex resume <id>` SUBCOMMAND.
///
/// The optional value is why this is a type and not a `&str`: emitting the flag
/// with no id opens an interactive session PICKER, which parks the pane forever
/// with no agent ever attaching — the exact stuck-agent failure
/// `diagnose_stuck_phase` exists to explain. So a surface is never composed
/// without an id: [`Config::resume_launch`] takes a [`SessionId`], which cannot
/// be empty or malformed, and there is no code path that emits the flag alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeSurface<'a> {
    /// `<command> … <flag> '<id>'`
    Flag(&'a str),
    /// `<command> <subcommand> '<id>' …`
    Subcommand(&'a str),
}

impl AgentSpec {
    /// The one resume surface this agent offers, or `None` when it offers none.
    ///
    /// Total by necessity — a match over two `Option`s has four arms — and the
    /// two arms that are not (flag) / (subcommand) / (neither) resolve to
    /// `None`: an agent configured with BOTH is ambiguous, and `load_config`
    /// already rejects it, so this is what the type requires rather than a
    /// second guard on the same rule. `None` degrades a rehydrate to a reseed,
    /// which is the safe direction.
    pub fn resume_surface(&self) -> Option<ResumeSurface<'_>> {
        match (
            self.resume_flag.as_deref().filter(|f| !f.trim().is_empty()),
            self.resume_subcommand
                .as_deref()
                .filter(|s| !s.trim().is_empty()),
        ) {
            (Some(flag), None) => Some(ResumeSurface::Flag(flag)),
            (None, Some(sub)) => Some(ResumeSurface::Subcommand(sub)),
            _ => None,
        }
    }
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
            // Verified against the real CLI: `claude -r, --resume [value]` —
            // a flag whose value is OPTIONAL.
            resume_flag: Some("--resume".into()),
            resume_subcommand: None,
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
            // Verified against the real CLI: `agent --resume [chatId]`.
            resume_flag: Some("--resume".into()),
            resume_subcommand: None,
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
            resume_flag: None,
            resume_subcommand: None,
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
        validate_resume(spec).map_err(|e| io::Error::other(format!("agent '{name}': {e}")))?;
    }
    for (name, builtin) in default_agents() {
        if let Some(spec) = config.agents.get_mut(&name) {
            // The resume surface merges as ONE unit, unlike every field below
            // it. Filling the two halves independently would graft the built-in
            // `--resume` onto an agent the user deliberately gave a
            // `resume_subcommand`, leaving a spec that claims both shapes —
            // which `validate_resume` has just rejected as ambiguous, except
            // that this happens after it. The user's resume surface is a whole
            // answer or no answer.
            if spec.resume_flag.is_none() && spec.resume_subcommand.is_none() {
                spec.resume_flag = builtin.resume_flag;
                spec.resume_subcommand = builtin.resume_subcommand;
            }
            spec.readonly_flag = spec.readonly_flag.take().or(builtin.readonly_flag);
            spec.workspace_flag = spec.workspace_flag.take().or(builtin.workspace_flag);
            spec.system_prompt_flag = spec
                .system_prompt_flag
                .take()
                .or(builtin.system_prompt_flag);
            spec.model_flag = spec.model_flag.take().or(builtin.model_flag);
            spec.review_model = spec.review_model.take().or(builtin.review_model);
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

    /// Compose an agent launch command pinned to `project_dir`, together with the
    /// backend it was composed from.
    ///
    /// Returns an [`AgentLaunch`] rather than a bare `String` so the command and
    /// the backend name cannot be passed around separately and drift — see that
    /// type for the bug that motivated it.
    pub fn launch(
        &self,
        agent: &str,
        project_dir: &str,
        readonly: bool,
    ) -> io::Result<AgentLaunch> {
        self.compose(agent, project_dir, readonly, None)
    }

    /// The one composer, shared by [`Config::launch`] and
    /// [`Config::resume_launch`] so a fresh launch and a resumed one cannot
    /// drift in their flags. `resume` decides only WHERE the id goes:
    /// a subcommand binds to the command and must precede every flag; a flag
    /// joins the flags.
    fn compose(
        &self,
        agent: &str,
        project_dir: &str,
        readonly: bool,
        resume: Option<(ResumeSurface<'_>, &SessionId)>,
    ) -> io::Result<AgentLaunch> {
        let spec = self.agent(agent)?;
        let mut command = spec.command.clone();
        if let Some((ResumeSurface::Subcommand(sub), session)) = resume {
            command.push(' ');
            command.push_str(sub);
            command.push(' ');
            command.push_str(&shell_single_quote(session.as_str()));
        }
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
        // LAST, and always with its id in the same push. The flag's value is
        // OPTIONAL to the agent, so the id is what separates "resume this
        // conversation" from "open the session picker and park forever".
        if let Some((ResumeSurface::Flag(flag), session)) = resume {
            command.push(' ');
            command.push_str(flag);
            command.push(' ');
            command.push_str(&shell_single_quote(session.as_str()));
        }
        Ok(AgentLaunch {
            backend: agent.to_owned(),
            command,
        })
    }

    /// Compose a launch that RESUMES `session` instead of starting a fresh
    /// conversation, or `Ok(None)` when `agent` offers no resume surface — in
    /// which case the caller must fall back to a plain [`Config::launch`] and
    /// re-seed the agent, because there is no way to ask this backend for its
    /// old session.
    ///
    /// `readonly` is re-passed exactly as it is for a fresh launch, and that is
    /// load-bearing for reviewers: a resumed reviewer launched without its
    /// `readonly_flag` is a second WRITER in a run that guarantees a single one.
    ///
    /// The id arrives as a [`SessionId`], whose alphabet
    /// (`[A-Za-z0-9._-]{1,128}`) is enforced at both of its constructors — so
    /// there is no "validate before composing" step to forget here, and no way
    /// to reach this function with an empty id and emit a bare flag.
    pub fn resume_launch(
        &self,
        agent: &str,
        project_dir: &str,
        readonly: bool,
        session: &SessionId,
    ) -> io::Result<Option<AgentLaunch>> {
        let spec = self.agent(agent)?;
        let Some(surface) = spec.resume_surface() else {
            return Ok(None);
        };
        self.compose(agent, project_dir, readonly, Some((surface, session)))
            .map(Some)
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

/// A composed agent invocation, inseparable from the backend that composed it.
///
/// The two travel together because they are one fact — "this pane runs THIS
/// agent, invoked THIS way" — and splitting them into two `&str` parameters put
/// a real bug in the tree: a caller passed a literal `"claude"` alongside a
/// command composed for a different backend, and the phase then recorded a
/// backend its pane was not running. Session capture checks a pane's session
/// against the recorded backend, so the mismatch silently captured nothing, for
/// exactly the panes (reviewers) whose session cannot be re-read later.
///
/// Only [`Config::launch`] constructs one, so the backend is always the name the
/// command was actually built from. There is no constructor that lets a caller
/// supply them independently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLaunch {
    backend: String,
    command: String,
}

impl AgentLaunch {
    /// The agent name (`claude`, `cursor`, …) this launch runs.
    pub fn backend(&self) -> &str {
        &self.backend
    }
    /// The full shell invocation to run in the pane.
    pub fn command(&self) -> &str {
        &self.command
    }
    /// Test-only: build a launch without a `Config`. The pairing this type
    /// exists to guarantee is a property of `Config::launch`, and a test that
    /// deliberately wants a cursor launch has no config to compose one from.
    #[cfg(test)]
    pub fn for_test(backend: &str, command: &str) -> AgentLaunch {
        AgentLaunch {
            backend: backend.to_owned(),
            command: command.to_owned(),
        }
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

/// Reject a resume surface that cannot compose a safe command line.
///
/// Two rules, both of which would otherwise surface as a pane that never
/// becomes ready:
///
/// * **Both shapes at once is ambiguous.** `resume_flag` and
///   `resume_subcommand` place the id in different positions; an agent claiming
///   both has no single answer, and picking one silently would be a guess about
///   the user's CLI.
/// * **An empty value is the bare-flag hazard, written down.** `resume_flag =
///   ""` composes `<command>  '<id>'` — the id as a positional argument — and a
///   whitespace-only one composes worse. Both are rejected at load, where the
///   user can see the message, rather than at compose time where the failure is
///   a pane parked on a session picker.
fn validate_resume(spec: &AgentSpec) -> io::Result<()> {
    if spec.resume_flag.is_some() && spec.resume_subcommand.is_some() {
        return Err(io::Error::other(
            "resume_flag and resume_subcommand are mutually exclusive; set exactly one",
        ));
    }
    for (key, value) in [
        ("resume_flag", &spec.resume_flag),
        ("resume_subcommand", &spec.resume_subcommand),
    ] {
        if value.as_deref().is_some_and(|v| v.trim().is_empty()) {
            return Err(io::Error::other(format!("{key} must not be empty")));
        }
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
            cfg.launch("cursor", "/tmp/my worktree", false)
                .unwrap()
                .command(),
            "agent --workspace '/tmp/my worktree'"
        );
        assert_eq!(
            cfg.launch("cursor", "/tmp/my worktree", true)
                .unwrap()
                .command(),
            "agent --mode plan --model 'composer-2.5' --workspace '/tmp/my worktree'"
        );
    }

    #[test]
    fn a_launch_carries_the_backend_it_was_composed_from() {
        // The command and the backend name are one fact, and splitting them into
        // two arguments put a real bug in the tree: a caller passed a literal
        // "claude" beside a command composed for another agent, so the phase
        // recorded a backend its pane was not running and session capture — which
        // checks a session against that backend — silently recorded nothing.
        //
        // `Config::launch` is the only constructor, so the pair cannot disagree:
        // the backend is always the name the command was built from. Note the
        // command here does not contain the string "cursor" anywhere — nothing
        // downstream could recover the backend by inspecting it.
        let cfg = Config::default();
        let launch = cfg.launch("cursor", "/tmp/p", true).unwrap();
        assert_eq!(launch.backend(), "cursor");
        assert!(
            launch.command().starts_with("agent "),
            "{}",
            launch.command()
        );
        assert!(
            !launch.command().contains("cursor"),
            "the backend is NOT recoverable from the command: {}",
            launch.command()
        );

        let claude = cfg.launch("claude", "/tmp/p", false).unwrap();
        assert_eq!(claude.backend(), "claude");
        assert!(claude.command().starts_with("claude"));
    }

    fn sid(value: &str) -> SessionId {
        SessionId::new(value.to_owned()).expect("test session id must be well-formed")
    }

    #[test]
    fn claude_and_cursor_resume_with_a_flag_and_codex_with_nothing() {
        let cfg = Config::default();
        assert_eq!(
            cfg.agents["claude"].resume_surface(),
            Some(ResumeSurface::Flag("--resume"))
        );
        assert_eq!(
            cfg.agents["cursor"].resume_surface(),
            Some(ResumeSurface::Flag("--resume"))
        );
        // codex gets NEITHER on purpose: `codex resume <id>` is the documented
        // shape but was never verified against the real CLI, and an unverified
        // guess composes a wrong command line where `None` merely reseeds.
        assert_eq!(cfg.agents["codex"].resume_surface(), None);
        assert_eq!(cfg.agents["codex"].resume_flag, None);
        assert_eq!(cfg.agents["codex"].resume_subcommand, None);
    }

    #[test]
    fn a_flag_resume_carries_its_id_and_never_appears_bare() {
        let cfg = Config::default();
        let launch = cfg
            .resume_launch("claude", "/tmp/p", false, &sid("abc-123.def_4"))
            .unwrap()
            .expect("claude offers a resume flag");
        assert_eq!(launch.backend(), "claude");
        assert!(
            launch.command().contains("--resume 'abc-123.def_4'"),
            "{}",
            launch.command()
        );
        // A bare `--resume` opens claude's interactive session picker, which
        // parks the pane forever. The id is never optional here.
        assert!(
            !launch.command().ends_with("--resume"),
            "never a bare flag: {}",
            launch.command()
        );
        assert!(
            !launch.command().contains("--resume  "),
            "never an empty id: {}",
            launch.command()
        );
        // Same project pinning a fresh launch gets: a session resolves under
        // `<profile>/projects/<escaped-cwd>/`, so the cwd must not drift.
        assert!(launch.command().contains("--add-dir '/tmp/p'"), "{}", launch.command());
    }

    #[test]
    fn a_resumed_reviewer_still_carries_its_readonly_flag() {
        // A resumed reviewer without its read-only flag is a second WRITER in a
        // run built on single-writer discipline.
        let cfg = Config::default();
        let launch = cfg
            .resume_launch("claude", "/tmp/p", true, &sid("sess-1"))
            .unwrap()
            .unwrap();
        assert!(
            launch.command().contains("--permission-mode plan"),
            "{}",
            launch.command()
        );
        assert!(launch.command().contains("--resume 'sess-1'"), "{}", launch.command());
    }

    #[test]
    fn a_resume_subcommand_comes_immediately_after_the_command() {
        let mut cfg = Config::default();
        let codex = cfg.agents.get_mut("codex").unwrap();
        codex.resume_subcommand = Some("resume".into());
        let launch = cfg
            .resume_launch("codex", "/tmp/p", false, &sid("sess-9"))
            .unwrap()
            .unwrap();
        // A subcommand is not a flag: it binds to the command and must precede
        // every flag, so the ORDER is the assertion, not mere presence.
        assert!(
            launch.command().starts_with("codex resume 'sess-9' "),
            "{}",
            launch.command()
        );
        assert!(launch.command().contains("-C '/tmp/p'"), "{}", launch.command());
    }

    #[test]
    fn an_agent_with_no_resume_surface_asks_the_caller_to_reseed() {
        // `Ok(None)`, not an error: "this backend cannot be resumed" is a normal
        // outcome that rehydrate answers with a fresh launch plus a re-seed.
        let cfg = Config::default();
        assert!(
            cfg.resume_launch("codex", "/tmp/p", false, &sid("sess-1"))
                .unwrap()
                .is_none()
        );
        // An unknown agent is still an error.
        assert!(cfg.resume_launch("nope", "/tmp/p", false, &sid("s")).is_err());
    }

    #[test]
    fn an_explicit_agent_block_keeps_the_builtin_resume_flag() {
        // The documented merge trap: a user config with an explicit
        // `[agents.claude]` block must not silently drop fields it omits.
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "[agents.claude]\ncommand = \"claude\"\n",
        )
        .unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        assert_eq!(
            cfg.agents["claude"].resume_surface(),
            Some(ResumeSurface::Flag("--resume"))
        );
    }

    #[test]
    fn an_explicit_resume_subcommand_is_not_joined_by_the_builtin_flag() {
        // The resume surface merges as ONE unit. Filling each field
        // independently would graft the built-in `--resume` onto an agent the
        // user deliberately gave a subcommand, producing a spec with both —
        // i.e. an ambiguous, unusable resume.
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "[agents.claude]\ncommand = \"claude\"\nresume_subcommand = \"resume\"\n",
        )
        .unwrap();
        set_config_home(tmp.path());

        let cfg = load_config().unwrap();
        assert_eq!(cfg.agents["claude"].resume_flag, None);
        assert_eq!(
            cfg.agents["claude"].resume_surface(),
            Some(ResumeSurface::Subcommand("resume"))
        );
    }

    #[test]
    fn an_agent_claiming_both_resume_shapes_is_rejected() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("drovr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "[agents.claude]\ncommand = \"claude\"\nresume_flag = \"--resume\"\n\
             resume_subcommand = \"resume\"\n",
        )
        .unwrap();
        set_config_home(tmp.path());

        let err = load_config().expect_err("both resume shapes must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("claude"), "error should name the agent: {msg}");
        assert!(
            msg.contains("resume_flag") && msg.contains("resume_subcommand"),
            "error should name both keys: {msg}"
        );

        // An empty flag is the bare-`--resume` hazard written into a config
        // file, and it is rejected at load rather than composed.
        std::fs::write(
            dir.join("config.toml"),
            "[agents.claude]\ncommand = \"claude\"\nresume_flag = \"\"\n",
        )
        .unwrap();
        assert!(load_config().is_err(), "an empty resume_flag must be rejected");
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
        assert!(!load_config().unwrap().worktree, "worktree = false must win");
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
            cfg.launch("codex", "/tmp/project", false)
                .unwrap()
                .command(),
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
                        resume_flag: None,
                        resume_subcommand: None,
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
                        resume_flag: None,
                        resume_subcommand: None,
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
                        resume_flag: None,
                        resume_subcommand: None,
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
        assert_eq!(
            cfg.reflex.preamble.as_deref(),
            Some("only a preamble here")
        );
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
